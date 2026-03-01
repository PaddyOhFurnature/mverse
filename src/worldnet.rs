/// WORLDNET OS — distributed operating system layer
///
/// Renders P2P content onto any physical surface (terminal, tablet, wall, billboard).
/// Same renderer, same address system, key-gated access tiers.

use crate::identity::KeyType;
use crate::meshsite::ContentItem;

// ── Address System ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum WorldnetAddress {
    Root,
    Signup,
    Forums,
    ForumThread(String),        // thread id
    Wiki,
    WikiPage(String),           // slug
    Marketplace,
    UserProfile(String),        // peer_id string
    WorldConstruct,
    WorldChunk(i32, i32),
    WorldObjects,
    Settings,
    SettingsIdentity,
    SettingsProperty,
    SettingsInventory,
    AdminKeys,
    AdminRegions,
    AdminModeration,
    AdminWorldConfig,
    Custom(String),             // worldnet://anything/else
}

impl WorldnetAddress {
    /// Parse a worldnet:// URL string into an address.
    pub fn parse(s: &str) -> Self {
        let path = s.strip_prefix("worldnet://").unwrap_or(s);
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        match parts.as_slice() {
            [] | [""] => Self::Root,
            ["signup"]                     => Self::Signup,
            ["forums"]                     => Self::Forums,
            ["forums", "thread", id]       => Self::ForumThread(id.to_string()),
            ["wiki"]                       => Self::Wiki,
            ["wiki", slug]                 => Self::WikiPage(slug.to_string()),
            ["marketplace"]                => Self::Marketplace,
            ["user", pid]                  => Self::UserProfile(pid.to_string()),
            ["world", "construct"]         => Self::WorldConstruct,
            ["world", "chunk", cx, cz]     => {
                let cx = cx.parse().unwrap_or(0);
                let cz = cz.parse().unwrap_or(0);
                Self::WorldChunk(cx, cz)
            }
            ["world", "objects"]           => Self::WorldObjects,
            ["settings"]                   => Self::Settings,
            ["settings", "identity"]       => Self::SettingsIdentity,
            ["settings", "property"]       => Self::SettingsProperty,
            ["settings", "inventory"]      => Self::SettingsInventory,
            ["admin", "keys"]              => Self::AdminKeys,
            ["admin", "regions"]           => Self::AdminRegions,
            ["admin", "moderation"]        => Self::AdminModeration,
            ["admin", "worldconfig"]       => Self::AdminWorldConfig,
            other                          => Self::Custom(other.join("/")),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Root                    => "worldnet://".into(),
            Self::Signup                  => "worldnet://signup".into(),
            Self::Forums                  => "worldnet://forums".into(),
            Self::ForumThread(id)         => format!("worldnet://forums/thread/{}", id),
            Self::Wiki                    => "worldnet://wiki".into(),
            Self::WikiPage(slug)          => format!("worldnet://wiki/{}", slug),
            Self::Marketplace             => "worldnet://marketplace".into(),
            Self::UserProfile(pid)        => format!("worldnet://user/{}", pid),
            Self::WorldConstruct          => "worldnet://world/construct".into(),
            Self::WorldChunk(cx, cz)      => format!("worldnet://world/chunk/{}/{}", cx, cz),
            Self::WorldObjects            => "worldnet://world/objects".into(),
            Self::Settings                => "worldnet://settings".into(),
            Self::SettingsIdentity        => "worldnet://settings/identity".into(),
            Self::SettingsProperty        => "worldnet://settings/property".into(),
            Self::SettingsInventory       => "worldnet://settings/inventory".into(),
            Self::AdminKeys               => "worldnet://admin/keys".into(),
            Self::AdminRegions            => "worldnet://admin/regions".into(),
            Self::AdminModeration         => "worldnet://admin/moderation".into(),
            Self::AdminWorldConfig        => "worldnet://admin/worldconfig".into(),
            Self::Custom(s)               => format!("worldnet://{}", s),
        }
    }

    /// Minimum key type required to access this address.
    pub fn required_key_type(&self) -> Option<KeyType> {
        match self {
            // Public — no key required
            Self::Root | Self::Signup | Self::Forums | Self::ForumThread(_)
            | Self::Wiki | Self::WikiPage(_) | Self::Marketplace
            | Self::WorldConstruct | Self::WorldChunk(..) | Self::WorldObjects => None,

            // Registered users only
            Self::UserProfile(_) | Self::Settings | Self::SettingsIdentity
            | Self::SettingsProperty | Self::SettingsInventory => Some(KeyType::Guest),

            // Admin only
            Self::AdminKeys | Self::AdminRegions
            | Self::AdminModeration | Self::AdminWorldConfig => Some(KeyType::Admin),

            Self::Custom(_) => None,
        }
    }

    /// Check whether a given key type can access this address.
    pub fn can_access(&self, key_type: Option<KeyType>) -> bool {
        match self.required_key_type() {
            None => true,
            Some(KeyType::Guest) => matches!(
                key_type,
                Some(KeyType::Guest) | Some(KeyType::User) | Some(KeyType::Business)
                | Some(KeyType::Admin) | Some(KeyType::Relay) | Some(KeyType::Server)
                | Some(KeyType::Genesis)
            ),
            Some(KeyType::Admin) => matches!(
                key_type,
                Some(KeyType::Admin) | Some(KeyType::Server) | Some(KeyType::Genesis)
            ),
            _ => true,
        }
    }
}

// ── Renderer ───────────────────────────────────────────────────────────────────

/// Resolution of the WORLDNET pixel buffer.
pub const WORLDNET_W: u32 = 512;
pub const WORLDNET_H: u32 = 384;

/// RGBA pixel buffer — upload directly to a wgpu Texture.
pub struct WorldnetPixelBuffer {
    pub pixels: Vec<u8>,   // RGBA, WORLDNET_W * WORLDNET_H * 4 bytes
    pub width:  u32,
    pub height: u32,
}

impl WorldnetPixelBuffer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0u8; (WORLDNET_W * WORLDNET_H * 4) as usize],
            width:  WORLDNET_W,
            height: WORLDNET_H,
        }
    }

    /// Fill the entire buffer with a solid colour (RGBA).
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for px in self.pixels.chunks_exact_mut(4) {
            px[0] = r; px[1] = g; px[2] = b; px[3] = a;
        }
    }

    /// Draw a filled rectangle.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u8, g: u8, b: u8) {
        for row in y..(y + h).min(self.height) {
            for col in x..(x + w).min(self.width) {
                let i = ((row * self.width + col) * 4) as usize;
                self.pixels[i]     = r;
                self.pixels[i + 1] = g;
                self.pixels[i + 2] = b;
                self.pixels[i + 3] = 255;
            }
        }
    }

    /// Draw a single 8×8 character using the font8x8 bitmap font.
    pub fn draw_char(&mut self, ch: char, x: u32, y: u32, r: u8, g: u8, b: u8) {
        use font8x8::UnicodeFonts;
        let glyph = font8x8::BASIC_FONTS.get(ch)
            .or_else(|| font8x8::BASIC_FONTS.get('?'));
        if let Some(glyph) = glyph {
            for (row, &bits) in glyph.iter().enumerate() {
                for col in 0..8u32 {
                    if bits & (1 << col) != 0 {
                        let px = x + col;
                        let py = y + row as u32;
                        if px < self.width && py < self.height {
                            let i = ((py * self.width + px) * 4) as usize;
                            self.pixels[i]     = r;
                            self.pixels[i + 1] = g;
                            self.pixels[i + 2] = b;
                            self.pixels[i + 3] = 255;
                        }
                    }
                }
            }
        }
    }

    /// Draw a string at pixel position (x, y). Returns x after last char.
    pub fn draw_text(&mut self, text: &str, x: u32, y: u32, r: u8, g: u8, b: u8) -> u32 {
        let mut cx = x;
        for ch in text.chars() {
            self.draw_char(ch, cx, y, r, g, b);
            cx += 9; // 8px glyph + 1px spacing
            if cx + 8 >= self.width { break; }
        }
        cx
    }

    /// Draw text with word-wrap. Returns y after last line.
    pub fn draw_text_wrapped(&mut self, text: &str, x: u32, y: u32, max_w: u32,
                              r: u8, g: u8, b: u8) -> u32 {
        let chars_per_line = (max_w / 9).max(1) as usize;
        let mut cy = y;
        let mut line = String::new();
        for word in text.split_whitespace() {
            if line.len() + word.len() + 1 > chars_per_line {
                self.draw_text(&line, x, cy, r, g, b);
                cy += 11;
                line = word.to_string();
            } else {
                if !line.is_empty() { line.push(' '); }
                line.push_str(word);
            }
            if cy >= self.height { break; }
        }
        if !line.is_empty() && cy < self.height {
            self.draw_text(&line, x, cy, r, g, b);
            cy += 11;
        }
        cy
    }
}

// ── Page Renderer ──────────────────────────────────────────────────────────────

/// Render a WORLDNET page to a pixel buffer.
pub fn render_page(
    addr:     &WorldnetAddress,
    key_type: Option<KeyType>,
    content:  &[ContentItem],
    buf:      &mut WorldnetPixelBuffer,
) {
    // Background: near-black
    buf.clear(12, 14, 20, 255);

    // Header bar
    buf.fill_rect(0, 0, buf.width, 14, 20, 24, 36);

    // Address bar text
    let addr_str = addr.to_string();
    buf.draw_text(&addr_str, 4, 3, 100, 180, 255);

    // Access denied
    if !addr.can_access(key_type) {
        buf.draw_text("ACCESS DENIED", 4, 30, 255, 60, 60);
        buf.draw_text("This address requires a higher key tier.", 4, 44, 180, 180, 180);
        return;
    }

    match addr {
        WorldnetAddress::Root | WorldnetAddress::WorldConstruct => {
            render_home(key_type, buf);
        }
        WorldnetAddress::Signup => {
            render_signup(buf);
        }
        WorldnetAddress::Forums | WorldnetAddress::Wiki
        | WorldnetAddress::Marketplace => {
            render_content_list(addr, content, buf);
        }
        WorldnetAddress::ForumThread(id) => {
            render_thread(id, content, buf);
        }
        WorldnetAddress::Settings | WorldnetAddress::SettingsIdentity => {
            render_settings(key_type, buf);
        }
        WorldnetAddress::AdminWorldConfig | WorldnetAddress::AdminKeys
        | WorldnetAddress::AdminRegions | WorldnetAddress::AdminModeration => {
            render_admin(addr, buf);
        }
        WorldnetAddress::Custom(s) if s == "help" => {
            render_help(buf);
        }
        _ => {
            render_not_found(addr, buf);
        }
    }
}

// ── Page implementations ───────────────────────────────────────────────────────

fn render_home(key_type: Option<KeyType>, buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("WORLDNET", 4, 20, 80, 220, 160);
    buf.draw_text("Decentralised platform", 4, 32, 140, 140, 140);
    buf.fill_rect(0, 44, buf.width, 1, 40, 44, 60);

    let mut y = 50u32;
    let items: &[(&str, &str)] = match key_type {
        None | Some(KeyType::Trial) => &[
            ("worldnet://forums",  "Forums       (read only)"),
            ("worldnet://wiki",    "Wiki         (read only)"),
            ("worldnet://signup",  "Sign up / Log in"),
        ],
        Some(KeyType::Guest) => &[
            ("worldnet://forums",      "Forums"),
            ("worldnet://wiki",        "Wiki"),
            ("worldnet://marketplace", "Marketplace"),
            ("worldnet://settings",    "Settings"),
        ],
        Some(KeyType::User) | Some(KeyType::Business) => &[
            ("worldnet://forums",      "Forums"),
            ("worldnet://wiki",        "Wiki"),
            ("worldnet://marketplace", "Marketplace"),
            ("worldnet://settings",    "Settings"),
        ],
        Some(KeyType::Admin) | Some(KeyType::Relay)
        | Some(KeyType::Server) | Some(KeyType::Genesis) => &[
            ("worldnet://forums",           "Forums"),
            ("worldnet://wiki",             "Wiki"),
            ("worldnet://marketplace",      "Marketplace"),
            ("worldnet://settings",         "Settings"),
            ("worldnet://admin/worldconfig","Admin: World Config"),
            ("worldnet://admin/moderation", "Admin: Moderation"),
        ],
    };
    for (addr, label) in items {
        buf.draw_text("›", 4, y, 80, 180, 255);
        buf.draw_text(label, 16, y, 220, 220, 220);
        buf.draw_text(addr, 16, y + 10, 60, 100, 140);
        y += 24;
    }
}

fn render_signup(buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("SIGN UP / LOG IN", 4, 20, 80, 220, 160);
    buf.fill_rect(0, 32, buf.width, 1, 40, 44, 60);
    let mut y = 38u32;
    for line in &[
        "Walk to the terminal in the lobby to",
        "create or load your identity key.",
        "",
        "Trial    — no registration, read only",
        "Guest    — free, email required",
        "User     — full access, certified",
    ] {
        let col = if line.starts_with("Trial") || line.starts_with("Guest") || line.starts_with("User") {
            (100u8, 200u8, 255u8)
        } else {
            (200u8, 200u8, 200u8)
        };
        buf.draw_text(line, 4, y, col.0, col.1, col.2);
        y += 12;
    }
}

fn render_content_list(addr: &WorldnetAddress, content: &[ContentItem], buf: &mut WorldnetPixelBuffer) {
    let title = match addr {
        WorldnetAddress::Forums      => "FORUMS",
        WorldnetAddress::Wiki        => "WIKI",
        WorldnetAddress::Marketplace => "MARKETPLACE",
        _ => "CONTENT",
    };
    buf.draw_text(title, 4, 20, 80, 220, 160);
    buf.fill_rect(0, 32, buf.width, 1, 40, 44, 60);

    if content.is_empty() {
        buf.draw_text("No content yet. Be the first to post.", 4, 42, 140, 140, 140);
        return;
    }

    let mut y = 38u32;
    for (i, item) in content.iter().enumerate().take(12) {
        let idx_str = format!("{:>2}.", i + 1);
        buf.draw_text(&idx_str, 4, y, 80, 140, 200);
        buf.draw_text(&item.title, 28, y, 220, 220, 220);
        let meta = format!("by {} ", &item.author[..item.author.len().min(12)]);
        buf.draw_text(&meta, 28, y + 10, 100, 120, 140);
        y += 24;
        if y + 24 > buf.height { break; }
    }
}

fn render_thread(id: &str, content: &[ContentItem], buf: &mut WorldnetPixelBuffer) {
    if let Some(item) = content.iter().find(|c| c.id == id) {
        buf.draw_text(&item.title, 4, 20, 80, 220, 160);
        let by = format!("by {}", item.author);
        buf.draw_text(&by, 4, 32, 100, 130, 160);
        buf.fill_rect(0, 44, buf.width, 1, 40, 44, 60);
        buf.draw_text_wrapped(&item.body, 4, 50, buf.width - 8, 210, 210, 210);
    } else {
        render_not_found(&WorldnetAddress::ForumThread(id.to_string()), buf);
    }
}

fn render_settings(key_type: Option<KeyType>, buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("SETTINGS", 4, 20, 80, 220, 160);
    buf.fill_rect(0, 32, buf.width, 1, 40, 44, 60);
    let tier = match key_type {
        None                      => "None (guest)",
        Some(KeyType::Trial)      => "Trial",
        Some(KeyType::Guest)      => "Guest",
        Some(KeyType::User)       => "Certified User",
        Some(KeyType::Business)   => "Business",
        Some(KeyType::Admin)      => "Admin",
        Some(KeyType::Relay)      => "Relay",
        Some(KeyType::Server)     => "Server",
        Some(KeyType::Genesis)    => "Genesis",
    };
    let line = format!("Key tier: {}", tier);
    buf.draw_text(&line, 4, 40, 200, 200, 200);
    buf.draw_text("worldnet://settings/identity", 4, 54, 80, 140, 200);
    buf.draw_text("worldnet://settings/property", 4, 66, 80, 140, 200);
    buf.draw_text("worldnet://settings/inventory", 4, 78, 80, 140, 200);
}

fn render_admin(addr: &WorldnetAddress, buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("ADMIN INTRANET", 4, 20, 255, 180, 60);
    buf.fill_rect(0, 32, buf.width, 1, 80, 60, 20);
    let page = match addr {
        WorldnetAddress::AdminWorldConfig  => "World Configuration",
        WorldnetAddress::AdminKeys         => "Key Registry",
        WorldnetAddress::AdminRegions      => "Region Management",
        WorldnetAddress::AdminModeration   => "Moderation Queue",
        _ => "Admin",
    };
    buf.draw_text(page, 4, 40, 255, 200, 100);
    buf.draw_text("(full editor — coming soon)", 4, 54, 140, 140, 140);
}

fn render_not_found(addr: &WorldnetAddress, buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("NOT FOUND", 4, 20, 255, 80, 80);
    let s = addr.to_string();
    buf.draw_text(&s, 4, 34, 140, 140, 140);
}

fn render_help(buf: &mut WorldnetPixelBuffer) {
    buf.draw_text("COMMANDS", 4, 20, 80, 220, 160);
    buf.fill_rect(0, 32, buf.width, 1, 40, 44, 60);
    let cmds: &[(&str, &str)] = &[
        ("forums / f",   "Go to forums"),
        ("wiki / w",     "Go to wiki"),
        ("market / m",   "Go to marketplace"),
        ("settings / s", "Identity & settings"),
        ("admin / a",    "Admin intranet"),
        ("home",         "Root page"),
        ("post",         "Compose a new post"),
        ("who",          "Your identity info"),
        ("go <path>",    "Navigate to worldnet://path"),
        ("exit / q",     "Close terminal"),
    ];
    let mut y = 40u32;
    for (cmd, desc) in cmds {
        buf.draw_text(cmd, 4, y, 100, 200, 255);
        buf.draw_text(desc, 112, y, 180, 180, 180);
        y += 12;
        if y + 12 > buf.height { break; }
    }
}

// ── Terminal Input ─────────────────────────────────────────────────────────────

/// Result of processing a typed terminal command.
#[derive(Debug, Clone)]
pub enum TerminalCmd {
    Navigate(WorldnetAddress),
    OpenCompose,
    Close,
    Refresh,
}

/// Parse a typed command string and return the action to take.
pub fn process_terminal_command(cmd: &str, _current: &WorldnetAddress) -> TerminalCmd {
    let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
    match parts.as_slice() {
        // Navigation shorthands
        [] | [""]              => TerminalCmd::Refresh,
        ["home"] | ["root"]    => TerminalCmd::Navigate(WorldnetAddress::Root),
        ["forums"] | ["f"]     => TerminalCmd::Navigate(WorldnetAddress::Forums),
        ["wiki"] | ["w"]       => TerminalCmd::Navigate(WorldnetAddress::Wiki),
        ["market"] | ["marketplace"] | ["m"]
                               => TerminalCmd::Navigate(WorldnetAddress::Marketplace),
        ["settings"] | ["s"]   => TerminalCmd::Navigate(WorldnetAddress::Settings),
        ["admin"] | ["a"]      => TerminalCmd::Navigate(WorldnetAddress::AdminWorldConfig),
        ["who"] | ["whoami"] | ["id"]
                               => TerminalCmd::Navigate(WorldnetAddress::SettingsIdentity),
        ["help"] | ["?"] | ["h"]
                               => TerminalCmd::Navigate(WorldnetAddress::Custom("help".into())),
        ["post"] | ["new"] | ["write"]
                               => TerminalCmd::OpenCompose,
        ["exit"] | ["quit"] | ["q"] | ["close"]
                               => TerminalCmd::Close,
        // Full address navigation: "go forums/thread/123" or "go worldnet://forums"
        ["go", path] | ["cd", path] | ["open", path] => {
            let addr_str = if path.starts_with("worldnet://") {
                path.to_string()
            } else {
                format!("worldnet://{}", path)
            };
            TerminalCmd::Navigate(WorldnetAddress::parse(&addr_str))
        }
        // Bare path: "forums/thread/abc"
        [path] => {
            let addr_str = if path.starts_with("worldnet://") {
                path.to_string()
            } else {
                format!("worldnet://{}", path)
            };
            TerminalCmd::Navigate(WorldnetAddress::parse(&addr_str))
        }
        _ => TerminalCmd::Refresh,
    }
}

/// Draw the active command prompt at the bottom of the pixel buffer.
/// Call this AFTER render_page() to overlay the prompt on top.
pub fn render_terminal_prompt(input: &str, buf: &mut WorldnetPixelBuffer) {
    let y = buf.height.saturating_sub(20);
    // Divider line
    buf.fill_rect(0, y, buf.width, 1, 40, 120, 80);
    // Prompt background
    buf.fill_rect(0, y + 1, buf.width, 19, 6, 8, 14);
    // Prompt symbol
    buf.draw_text(">", 4, y + 6, 60, 200, 120);
    // Input text (cap display at buffer width)
    let display: String = input.chars().rev().take(((buf.width - 20) / 9) as usize).collect::<String>().chars().rev().collect();
    buf.draw_text(&display, 16, y + 6, 240, 240, 240);
    // Block cursor at end of input
    let cursor_x = 16 + (display.len() as u32) * 9;
    if cursor_x + 8 < buf.width {
        buf.fill_rect(cursor_x, y + 5, 7, 10, 60, 200, 120);
    }
}

/// Helper: return the content-section key for a given address.
pub fn addr_section(addr: &WorldnetAddress) -> &'static str {
    match addr {
        WorldnetAddress::Forums | WorldnetAddress::ForumThread(_) => "forums",
        WorldnetAddress::Wiki   | WorldnetAddress::WikiPage(_)    => "wiki",
        WorldnetAddress::Marketplace                               => "marketplace",
        _                                                          => "",
    }
}
