//! Metaverse P2P Relay Server
//!
//! TUI dashboard by default when run in a terminal.
//! Automatically falls back to plain log when piped/redirected.
//! Use --headless to force plain log mode (set-and-forget servers).
//!
//! Config: ./relay.json or ~/.metaverse/relay.json (local takes priority).
//! CLI args override config file values.
//!
//! Keybindings: [m] Main  [l] Log  [h] Help  [q] Quit

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use libp2p::{
    identify, identity, kad, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, SwarmBuilder,
};
use libp2p::kad::store::MemoryStore;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use sysinfo::System;
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    io::{self, IsTerminal},
    time::{Duration, Instant},
};
use clap::Parser;

// ─── Config ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct RelayConfig {
    pub port: u16,
    pub external_addr: Option<String>,
    pub max_circuits: usize,
    pub max_circuit_duration: u64,
    pub max_circuit_bytes: u64,
    pub peers: Vec<String>,
    pub blacklist: Vec<String>,
    pub whitelist: Vec<String>,
    pub priority_peers: Vec<String>,
    pub node_name: Option<String>,
    pub headless: bool,
    pub ui: UiConfig,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            port: 4001,
            external_addr: None,
            max_circuits: 100,
            max_circuit_duration: 3600,
            max_circuit_bytes: 1_073_741_824,
            peers: vec![],
            blacklist: vec![],
            whitelist: vec![],
            priority_peers: vec![],
            node_name: None,
            headless: false,
            ui: UiConfig::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UiConfig {
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_dht: bool,
    pub refresh_ms: u64,
    pub max_log_entries: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { show_cpu: true, show_ram: true, show_dht: true, refresh_ms: 500, max_log_entries: 500 }
    }
}

fn load_config() -> RelayConfig {
    let paths = [
        std::path::PathBuf::from("relay.json"),
        dirs::home_dir().unwrap_or_default().join(".metaverse").join("relay.json"),
    ];
    for path in &paths {
        if path.exists() {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(cfg) = serde_json::from_str::<RelayConfig>(&text) {
                    return cfg;
                }
            }
        }
    }
    RelayConfig::default()
}

fn write_default_config_if_missing() {
    let path = dirs::home_dir().unwrap_or_default().join(".metaverse").join("relay.json");
    if !path.exists() {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p).ok(); }
        if let Ok(json) = serde_json::to_string_pretty(&RelayConfig::default()) {
            std::fs::write(&path, json).ok();
        }
    }
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "metaverse-relay")]
#[command(about = "Metaverse P2P relay — TUI dashboard, or --headless for plain log")]
struct Args {
    /// TCP port (WebSocket = port+5000)
    #[arg(short, long)]
    port: Option<u16>,
    #[arg(long)]
    external_addr: Option<String>,
    #[arg(long)]
    max_circuits: Option<usize>,
    #[arg(long)]
    max_circuit_duration: Option<u64>,
    #[arg(long)]
    max_circuit_bytes: Option<u64>,
    /// Peer relay to connect to at startup (repeatable)
    #[arg(long)]
    peer: Vec<String>,
    /// Plain log output, no TUI (auto-detected when not a terminal)
    #[arg(long)]
    headless: bool,
    /// Node display name shown in the dashboard header
    #[arg(long)]
    name: Option<String>,
}

fn apply_cli_overrides(config: &mut RelayConfig, args: Args) {
    if let Some(v) = args.port                 { config.port = v; }
    if let Some(v) = args.external_addr        { config.external_addr = Some(v); }
    if let Some(v) = args.max_circuits         { config.max_circuits = v; }
    if let Some(v) = args.max_circuit_duration { config.max_circuit_duration = v; }
    if let Some(v) = args.max_circuit_bytes    { config.max_circuit_bytes = v; }
    if !args.peer.is_empty()                   { config.peers.extend(args.peer); }
    if args.headless                            { config.headless = true; }
    if let Some(v) = args.name                 { config.node_name = Some(v); }
}

// ─── Network behaviour ───────────────────────────────────────────────────────

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: libp2p::ping::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
}

// ─── App state ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct PeerInfo { peer_id: PeerId, connected_at: Instant, addr: String }

#[derive(Clone)]
struct CircuitInfo { src: PeerId, dst: PeerId, started_at: Instant }

#[derive(PartialEq, Clone)]
enum Tab { Main, Log, Help }

struct AppState {
    config: RelayConfig,
    local_peer_id: String,
    public_ip: String,
    start_time: Instant,
    connected_peers: HashMap<PeerId, PeerInfo>,
    active_circuits: Vec<CircuitInfo>,
    total_connections: u64,
    total_reservations: u64,
    dht_peer_count: usize,
    log: VecDeque<String>,
    tab: Tab,
    should_quit: bool,
    sys: System,
    cpu_pct: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    last_sys_refresh: Instant,
}

impl AppState {
    fn new(config: RelayConfig, local_peer_id: String, public_ip: String) -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let cpu_pct = sys.global_cpu_usage();
        let ram_used_mb = sys.used_memory() / 1_048_576;
        let ram_total_mb = sys.total_memory() / 1_048_576;
        Self {
            config, local_peer_id, public_ip,
            start_time: Instant::now(),
            connected_peers: HashMap::new(),
            active_circuits: vec![],
            total_connections: 0, total_reservations: 0, dht_peer_count: 0,
            log: VecDeque::new(), tab: Tab::Main, should_quit: false,
            sys, cpu_pct, ram_used_mb, ram_total_mb,
            last_sys_refresh: Instant::now(),
        }
    }

    fn push_log(&mut self, msg: String) {
        let e = self.start_time.elapsed().as_secs();
        let entry = format!("{:02}:{:02}:{:02} {}", e/3600, (e%3600)/60, e%60, msg);
        self.log.push_back(entry.clone());
        while self.log.len() > self.config.ui.max_log_entries { self.log.pop_front(); }
        if self.config.headless { println!("{}", entry); }
    }

    fn refresh_sys(&mut self) {
        if self.last_sys_refresh.elapsed() > Duration::from_secs(2) {
            self.sys.refresh_cpu_usage();
            self.sys.refresh_memory();
            self.cpu_pct = self.sys.global_cpu_usage();
            self.ram_used_mb = self.sys.used_memory() / 1_048_576;
            self.ram_total_mb = self.sys.total_memory() / 1_048_576;
            self.last_sys_refresh = Instant::now();
        }
    }

    fn short(id: &str) -> String {
        if id.len() > 12 { format!("…{}", &id[id.len()-10..]) } else { id.to_string() }
    }
}

// ─── Swarm event handler ─────────────────────────────────────────────────────

enum SwarmAction { AddKadAddress(PeerId, Multiaddr), RefreshDhtCount }

fn handle_swarm_event(event: SwarmEvent<RelayBehaviourEvent>, state: &mut AppState) -> Vec<SwarmAction> {
    let mut actions = vec![];
    match event {
        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
            state.total_connections += 1;
            let addr = endpoint.get_remote_address().to_string();
            state.connected_peers.insert(peer_id, PeerInfo { peer_id, connected_at: Instant::now(), addr: addr.clone() });
            state.push_log(format!("🔗 Connected: {} via {}", AppState::short(&peer_id.to_string()), addr));
        }
        SwarmEvent::ConnectionClosed { peer_id, num_established, cause, .. } => {
            if num_established == 0 {
                state.connected_peers.remove(&peer_id);
                let r = cause.map(|e| e.to_string()).unwrap_or_default();
                let r = if r.len() > 50 { r[..50].to_string() } else { r };
                state.push_log(format!("❌ Disconnected: {} {}", AppState::short(&peer_id.to_string()), r));
            }
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            state.push_log(format!("👂 Listening: {}", address));
        }
        SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(ev)) => match ev {
            relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                state.total_reservations += 1;
                state.push_log(format!("✅ Reservation: {}", AppState::short(&src_peer_id.to_string())));
            }
            relay::Event::ReservationTimedOut { src_peer_id } => {
                state.push_log(format!("⏱️  Expired: {}", AppState::short(&src_peer_id.to_string())));
            }
            relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                state.active_circuits.push(CircuitInfo { src: src_peer_id, dst: dst_peer_id, started_at: Instant::now() });
                state.push_log(format!("🔄 Circuit: {} → {}", AppState::short(&src_peer_id.to_string()), AppState::short(&dst_peer_id.to_string())));
            }
            relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                state.active_circuits.retain(|c| !(c.src == src_peer_id && c.dst == dst_peer_id));
                state.push_log(format!("🔚 Circuit closed: {} → {}", AppState::short(&src_peer_id.to_string()), AppState::short(&dst_peer_id.to_string())));
            }
            _ => {}
        },
        SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) => {
            for addr in info.listen_addrs { actions.push(SwarmAction::AddKadAddress(peer_id, addr)); }
            actions.push(SwarmAction::RefreshDhtCount);
        }
        _ => {}
    }
    actions
}

fn apply_swarm_actions(actions: Vec<SwarmAction>, state: &mut AppState, swarm: &mut libp2p::Swarm<RelayBehaviour>) {
    for action in actions {
        match action {
            SwarmAction::AddKadAddress(peer_id, addr) => {
                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
            }
            SwarmAction::RefreshDhtCount => {
                state.dht_peer_count = swarm.behaviour_mut().kademlia.kbuckets().map(|b| b.num_entries()).sum();
            }
        }
    }
}

// ─── TUI rendering ───────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, state: &AppState) {
    match state.tab {
        Tab::Main => draw_main(frame, state),
        Tab::Log  => draw_log_full(frame, state),
        Tab::Help => draw_help(frame, state),
    }
}

fn draw_main(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8), Constraint::Length(7), Constraint::Length(1)])
        .split(area);

    draw_header(frame, state, outer[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(outer[1]);
    draw_peers(frame, state, body[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body[1]);
    draw_circuits(frame, state, right[0]);
    draw_stats(frame, state, right[1]);

    draw_log_tail(frame, state, outer[2]);
    draw_hints(frame, outer[3]);
}

fn draw_header(frame: &mut Frame, state: &AppState, area: Rect) {
    let e = state.start_time.elapsed().as_secs();
    let name = state.config.node_name.as_deref().unwrap_or("relay");
    let text = format!(
        " 🌐 {}  │  {}:{}  │  {}  │  ⏱ {:02}h{:02}m{:02}s  │  peers: {}  circuits: {} ",
        name, state.public_ip, state.config.port, AppState::short(&state.local_peer_id),
        e/3600, (e%3600)/60, e%60,
        state.connected_peers.len(), state.active_circuits.len()
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_peers(frame: &mut Frame, state: &AppState, area: Rect) {
    let rows: Vec<Row> = state.connected_peers.values().map(|p| {
        let age = p.connected_at.elapsed().as_secs();
        let age_s = if age < 60 { format!("{}s", age) } else { format!("{}m", age/60) };
        let addr = if p.addr.len() > 32 { format!("…{}", &p.addr[p.addr.len()-30..]) } else { p.addr.clone() };
        Row::new(vec![
            Cell::from(AppState::short(&p.peer_id.to_string())).style(Style::default().fg(Color::Green)),
            Cell::from(addr).style(Style::default().fg(Color::DarkGray)),
            Cell::from(age_s).style(Style::default().fg(Color::Yellow)),
        ])
    }).collect();
    frame.render_widget(
        Table::new(rows, [Constraint::Min(14), Constraint::Min(28), Constraint::Length(6)])
            .header(Row::new(["Peer", "Address", "Age"])
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)))
            .block(Block::default()
                .title(format!(" Connected ({}) ", state.connected_peers.len()))
                .title_style(Style::default().fg(Color::Cyan))
                .borders(Borders::ALL)),
        area,
    );
}

fn draw_circuits(frame: &mut Frame, state: &AppState, area: Rect) {
    let items: Vec<ListItem> = state.active_circuits.iter().map(|c| {
        ListItem::new(Line::from(vec![
            Span::styled(AppState::short(&c.src.to_string()), Style::default().fg(Color::Green)),
            Span::raw(" → "),
            Span::styled(AppState::short(&c.dst.to_string()), Style::default().fg(Color::Blue)),
            Span::styled(format!(" {}s", c.started_at.elapsed().as_secs()), Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();
    frame.render_widget(
        List::new(items).block(Block::default()
            .title(format!(" Circuits ({}) ", state.active_circuits.len()))
            .title_style(Style::default().fg(Color::Cyan))
            .borders(Borders::ALL)),
        area,
    );
}

fn draw_stats(frame: &mut Frame, state: &AppState, area: Rect) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    frame.render_widget(Paragraph::new(vec![
        stat_line("Conns total: ", &format!("{}", state.total_connections)),
        stat_line("Reservations:", &format!("{}", state.total_reservations)),
        stat_line("DHT peers:   ", &format!("{}", state.dht_peer_count)),
        stat_line("Peer relays: ", &format!("{}", state.config.peers.len())),
    ]).block(Block::default().title(" Network ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL)),
    halves[0]);

    frame.render_widget(Paragraph::new(vec![
        Line::from(vec![
            Span::styled("CPU:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:.1}%", state.cpu_pct), cpu_color(state.cpu_pct)),
        ]),
        stat_line("RAM:  ", &format!("{}/{}MB", state.ram_used_mb, state.ram_total_mb)),
        stat_line("Port: ", &format!("{}  WS:{}", state.config.port, state.config.port+5000)),
        stat_line("Limit:", &format!("{} circuits", state.config.max_circuits)),
    ]).block(Block::default().title(" System ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL)),
    halves[1]);
}

fn draw_log_tail(frame: &mut Frame, state: &AppState, area: Rect) {
    let h = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = state.log.iter().rev().take(h).rev().map(|e| log_style(e)).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Log [l] ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_log_full(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let h = parts[0].height.saturating_sub(2) as usize;
    let lines: Vec<Line> = state.log.iter().rev().take(h).rev().map(|e| log_style(e)).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Full Log ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        parts[0],
    );
    draw_hints(frame, parts[1]);
}

fn draw_help(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let cfg = dirs::home_dir().unwrap_or_default().join(".metaverse").join("relay.json");
    let lines = vec![
        Line::from(Span::styled(" Metaverse Relay — Help ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(" Keys", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  m / Esc    Main dashboard"),
        Line::from("  l          Full log"),
        Line::from("  h          This help"),
        Line::from("  q / Ctrl+C  Quit"),
        Line::from(""),
        Line::from(Span::styled(" Config file (edit + restart to apply)", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(format!("  {}", cfg.display())),
        Line::from("  port, external_addr, max_circuits, max_circuit_duration, max_circuit_bytes,"),
        Line::from("  peers[], blacklist[], whitelist[], priority_peers[], node_name, headless,"),
        Line::from("  ui { show_cpu, show_ram, show_dht, refresh_ms, max_log_entries }"),
        Line::from(""),
        Line::from(Span::styled(" CLI flags (override config)", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  --port N             TCP port (WebSocket = N+5000)"),
        Line::from("  --peer MULTIADDR     Peer with another relay at startup"),
        Line::from("  --headless           Plain log output, no TUI"),
        Line::from("  --name NAME          Node display name"),
        Line::from("  --max-circuits N     Circuit limit"),
        Line::from(""),
        Line::from(Span::styled(" This node", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(format!("  Peer ID: {}", state.local_peer_id)),
        Line::from(format!("  Public:  {}:{}", state.public_ip, state.config.port)),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        parts[0],
    );
    draw_hints(frame, parts[1]);
}

fn draw_hints(frame: &mut Frame, area: Rect) {
    frame.render_widget(Paragraph::new(Line::from(vec![
        Span::styled(" [m]", Style::default().fg(Color::Yellow)), Span::raw(" Main  "),
        Span::styled("[l]", Style::default().fg(Color::Yellow)), Span::raw(" Log  "),
        Span::styled("[h]", Style::default().fg(Color::Yellow)), Span::raw(" Help  "),
        Span::styled("[q]", Style::default().fg(Color::Red)), Span::raw(" Quit"),
    ])).style(Style::default().fg(Color::DarkGray)), area);
}

fn stat_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::styled(value, Style::default().fg(Color::White)),
    ])
}

fn log_style(line: &str) -> Line {
    let color = if line.contains("✅") || line.contains("Reservation") { Color::Green }
        else if line.contains("❌") || line.contains("Disconnected") { Color::Red }
        else if line.contains("🔄") || line.contains("Circuit") { Color::Blue }
        else if line.contains("🔗") || line.contains("Connected") { Color::Cyan }
        else if line.contains("⏱") { Color::Yellow }
        else { Color::DarkGray };
    Line::from(Span::styled(line, Style::default().fg(color)))
}

fn cpu_color(pct: f32) -> Style {
    if pct > 80.0 { Style::default().fg(Color::Red) }
    else if pct > 50.0 { Style::default().fg(Color::Yellow) }
    else { Style::default().fg(Color::Green) }
}

// ─── Public IP detection ─────────────────────────────────────────────────────

async fn detect_public_ip() -> Option<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().ok()?;
    for url in &["https://api.ipify.org", "https://ipv4.icanhazip.com", "https://checkip.amazonaws.com"] {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(text) = resp.text().await {
                let ip = text.trim().to_string();
                if ip.split('.').count() == 4 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return Some(ip);
                }
            }
        }
    }
    None
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut config = load_config();
    apply_cli_overrides(&mut config, args);
    write_default_config_if_missing();

    // Headless if flag/config set, or stdout is not a terminal (piped/redirected)
    let headless = config.headless || !io::stdout().is_terminal();
    config.headless = headless;

    // Load or generate persistent identity
    let key_path = dirs::home_dir().unwrap_or_default().join(".metaverse").join("relay.key");
    std::fs::create_dir_all(key_path.parent().unwrap())?;
    let local_key = if key_path.exists() {
        identity::Keypair::from_protobuf_encoding(&std::fs::read(&key_path)?)?
    } else {
        let kp = identity::Keypair::generate_ed25519();
        std::fs::write(&key_path, kp.to_protobuf_encoding()?)?;
        kp
    };
    let local_peer_id = local_key.public().to_peer_id();
    println!("🔑 Peer ID: {}", local_peer_id);

    // Public IP
    let public_ip = if let Some(ref addr) = config.external_addr {
        addr.split('/').find(|s| s.parse::<std::net::Ipv4Addr>().is_ok()).unwrap_or("?").to_string()
    } else {
        print!("🌐 Detecting public IP... ");
        let ip = detect_public_ip().await.unwrap_or_else(|| "unknown".to_string());
        println!("{}", ip);
        ip
    };

    // Build swarm
    let max_circuits = config.max_circuits;
    let max_circuit_duration = Duration::from_secs(config.max_circuit_duration);
    let max_circuit_bytes = config.max_circuit_bytes;
    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(libp2p::tcp::Config::default(), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_dns()?
        .with_websocket((libp2p::tls::Config::new, libp2p::noise::Config::new), libp2p::yamux::Config::default)
        .await?
        .with_behaviour(|key: &identity::Keypair| {
            let peer_id = key.public().to_peer_id();
            let mut kad_config = kad::Config::default();
            kad_config.set_query_timeout(Duration::from_secs(60));
            let mut kademlia = kad::Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kad_config);
            kademlia.set_mode(Some(kad::Mode::Server));
            let identify = identify::Behaviour::new(
                identify::Config::new("/metaverse/1.0.0".to_string(), key.public())
                    .with_push_listen_addr_updates(true),
            );
            Ok(RelayBehaviour {
                relay: relay::Behaviour::new(peer_id, relay::Config {
                    max_reservations: max_circuits,
                    max_circuits,
                    max_circuit_duration,
                    max_circuit_bytes,
                    ..Default::default()
                }),
                ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
                kademlia,
                identify,
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let ws_port = config.port + 5000;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", config.port).parse()?)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}/ws", ws_port).parse()?)?;

    let ext_tcp = config.external_addr.clone()
        .unwrap_or_else(|| format!("/ip4/{}/tcp/{}", public_ip, config.port));
    let ext_ws = format!("/ip4/{}/tcp/{}/ws", public_ip, ws_port);
    if let Ok(a) = ext_tcp.parse() { swarm.add_external_address(a); }
    if let Ok(a) = ext_ws.parse()  { swarm.add_external_address(a); }

    // Dial peer relays
    for peer_addr in config.peers.clone() {
        if let Ok(addr) = peer_addr.parse::<Multiaddr>() {
            if let Some(libp2p::multiaddr::Protocol::P2p(pid)) = addr.iter().last() {
                swarm.behaviour_mut().kademlia.add_address(&pid, addr.clone());
            }
            match swarm.dial(addr) {
                Ok(()) => println!("🔗 Dialing peer: {}", peer_addr),
                Err(e) => eprintln!("✗ {}: {}", peer_addr, e),
            }
        }
    }
    if !config.peers.is_empty() { swarm.behaviour_mut().kademlia.bootstrap().ok(); }

    let mut state = AppState::new(config, local_peer_id.to_string(), public_ip);
    state.push_log(format!("✅ Relay started — {}", local_peer_id));

    if headless { run_headless(swarm, state).await } else { run_tui(swarm, state).await }
}

// ─── Headless loop ───────────────────────────────────────────────────────────

async fn run_headless(mut swarm: libp2p::Swarm<RelayBehaviour>, mut state: AppState) -> Result<(), Box<dyn Error>> {
    loop {
        let event = swarm.select_next_some().await;
        let actions = handle_swarm_event(event, &mut state);
        apply_swarm_actions(actions, &mut state, &mut swarm);
    }
}

// ─── TUI loop ────────────────────────────────────────────────────────────────

async fn run_tui(mut swarm: libp2p::Swarm<RelayBehaviour>, mut state: AppState) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(state.config.ui.refresh_ms));

    let result: Result<(), Box<dyn Error>> = async {
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    state.refresh_sys();
                    terminal.draw(|f| draw(f, &state))?;
                }
                maybe_ev = events.next() => {
                    if let Some(Ok(Event::Key(k))) = maybe_ev {
                        match (k.code, k.modifiers) {
                            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                state.should_quit = true;
                            }
                            (KeyCode::Char('l'), _) => { state.tab = Tab::Log; }
                            (KeyCode::Char('h'), _) => { state.tab = Tab::Help; }
                            (KeyCode::Char('m'), _) | (KeyCode::Esc, _) => { state.tab = Tab::Main; }
                            _ => {}
                        }
                    }
                }
                swarm_event = swarm.select_next_some() => {
                    let actions = handle_swarm_event(swarm_event, &mut state);
                    apply_swarm_actions(actions, &mut state, &mut swarm);
                }
            }
            if state.should_quit { break; }
        }
        Ok(())
    }.await;

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    result
}
