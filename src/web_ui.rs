//! Shared web dashboard module — server, relay, and (optionally) client.
//!
//! Provides:
//! - [`NodeStatus`]: serialisable snapshot of any node's runtime state.
//! - [`build_base_router`]: Axum router with the dashboard UI and common API endpoints.
//!
//! Each binary extends the base router with its own routes:
//! ```text
//! let app = web_ui::build_base_router(status_arc)
//!     .route("/api/v1/keys", get(my_keys_handler))
//!     .with_state(my_other_state);
//! ```

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Data types ────────────────────────────────────────────────────────────────

/// Summary of one connected peer, safe to serialise and send to the web UI.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerSummary {
    pub peer_id:        String,
    pub peer_type:      String,   // "client" | "relay" | "server" | "unknown"
    pub addr:           String,
    pub connected_secs: u64,
}

/// Serialisable snapshot of a node's current state.
/// Populated by server / relay / client and served at `/api/status`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_name:        String,
    pub node_type:        String,   // "server" | "relay" | "client"
    pub version:          String,
    pub peer_id:          String,
    pub public_ip:        String,
    pub p2p_port:         u16,
    pub web_port:         u16,
    pub uptime_secs:      u64,
    pub peers:            Vec<PeerSummary>,
    pub circuit_count:    usize,
    pub total_connections: u64,
    pub dht_peer_count:   usize,
    pub gossip_msgs_in:   u64,
    pub gossip_msgs_out:  u64,
    pub bytes_in:         u64,
    pub bytes_out:        u64,
    pub cpu_pct:          f32,
    pub ram_used_mb:      u64,
    pub ram_total_mb:     u64,
    pub shedding:         bool,
    /// Node-type-specific extras (e.g. world stats, relay reservations).
    /// Serialised as a JSON object; the web UI reads known sub-keys.
    #[serde(default)]
    pub extra:            serde_json::Value,
}

/// Shared handle used as Axum router state.
pub type SharedStatus = Arc<RwLock<NodeStatus>>;

// ── Router builder ────────────────────────────────────────────────────────────

/// Build the base Axum router shared by all node types.
///
/// Provides: `GET /`, `GET /health`, `GET /api/status`, `GET /api/peers`.
///
/// Extend with node-specific routes using `.route(...)` or `.merge(...)` before
/// calling `.with_state(...)` on the returned router (or just add them before
/// this call — the `with_state` on this router is already applied internally).
pub fn build_base_router(status: SharedStatus) -> Router {
    Router::new()
        .route("/",           get(web_root))
        .route("/health",     get(web_health))
        .route("/api/status", get(web_api_status))
        .route("/api/peers",  get(web_api_peers))
        .with_state(status)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn web_root() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

async fn web_health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn web_api_status(State(s): State<SharedStatus>) -> impl IntoResponse {
    Json(s.read().await.clone())
}

async fn web_api_peers(State(s): State<SharedStatus>) -> impl IntoResponse {
    Json(s.read().await.peers.clone())
}

// ── Embedded Pi-Hole-style dashboard ─────────────────────────────────────────

pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Metaverse Node</title>
<style>
:root{--bg:#0d1117;--surface:#161b22;--border:#30363d;--cyan:#58a6ff;--green:#3fb950;--red:#f85149;--yellow:#d29922;--text:#e6edf3;--dim:#8b949e;--accent:#1f6feb}
*{box-sizing:border-box;margin:0;padding:0}
body{background:var(--bg);color:var(--text);font:13px/1.5 'Courier New',monospace;display:flex;height:100vh;overflow:hidden}
aside{width:210px;min-width:210px;background:var(--surface);border-right:1px solid var(--border);display:flex;flex-direction:column}
.logo{padding:14px 18px;border-bottom:1px solid var(--border);color:var(--cyan);font-size:1.05em;font-weight:bold;letter-spacing:.04em}
.logo em{color:var(--dim);font-style:normal;font-size:.72em;display:block;margin-top:2px}
nav{flex:1;padding:6px 0;overflow-y:auto}
nav a{display:flex;align-items:center;gap:9px;padding:7px 18px;color:var(--dim);text-decoration:none;cursor:pointer;font-size:.88em;transition:color .1s,background .1s}
nav a:hover{color:var(--text);background:rgba(255,255,255,.04)}
nav a.active{color:var(--cyan);background:rgba(88,166,255,.08);border-left:2px solid var(--cyan);padding-left:16px}
nav a .ic{width:14px;text-align:center;flex-shrink:0}
.node-meta{padding:10px 18px;border-top:1px solid var(--border);font-size:.76em;color:var(--dim)}
.node-meta .nm{color:var(--text);font-weight:bold;margin-bottom:3px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.nt{display:inline-block;padding:1px 7px;border-radius:9px;font-size:.82em;margin-bottom:3px}
.nt-server{background:#0f3460;color:var(--cyan)}.nt-relay{background:#1a3a5c;color:#74b9ff}
.nt-client{background:#1a3a1a;color:var(--green)}.nt-unknown{background:#333;color:var(--dim)}
main{flex:1;display:flex;flex-direction:column;overflow:hidden}
.topbar{padding:10px 20px;border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between;flex-shrink:0}
.topbar h2{font-size:.95em;color:var(--text);font-weight:bold}
.live{font-size:.75em;padding:2px 8px;border-radius:9px}
.live.ok{background:rgba(63,185,80,.15);color:var(--green)}.live.err{background:rgba(248,81,73,.15);color:var(--red)}
#content{flex:1;overflow-y:auto;padding:18px 20px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(195px,1fr));gap:10px;margin-bottom:18px}
.card{background:var(--surface);border:1px solid var(--border);border-radius:6px;padding:13px}
.card-title{color:var(--dim);font-size:.72em;text-transform:uppercase;letter-spacing:.08em;margin-bottom:9px}
.stat{display:flex;justify-content:space-between;align-items:center;padding:3px 0;border-bottom:1px solid rgba(48,54,61,.5)}
.stat:last-child{border:none}.stat .lbl{color:var(--dim);font-size:.84em}.stat .val{color:var(--cyan)}
.ok{color:var(--green)}.warn{color:var(--yellow)}.err{color:var(--red)}
.bw{background:var(--border);border-radius:3px;height:5px;margin-top:5px;overflow:hidden}
.bw .bf{height:5px;border-radius:3px;transition:width .7s}
table{width:100%;border-collapse:collapse;font-size:.86em}
th{color:var(--dim);text-align:left;padding:5px 9px;border-bottom:1px solid var(--border);font-weight:normal;font-size:.78em;text-transform:uppercase;letter-spacing:.05em}
td{padding:5px 9px;border-bottom:1px solid rgba(48,54,61,.4)}
tr:hover td{background:rgba(255,255,255,.02)}
.badge{padding:1px 7px;border-radius:9px;font-size:.76em}
.badge-server{background:#0f3460;color:var(--cyan)}.badge-relay{background:#1a3a5c;color:#74b9ff}
.badge-client{background:#1a3a1a;color:var(--green)}.badge-unknown{background:#333;color:var(--dim)}
.sec{background:var(--surface);border:1px solid var(--border);border-radius:6px;padding:13px;margin-bottom:14px}
pre{color:var(--dim);font-size:.8em;overflow:auto;max-height:55vh;white-space:pre-wrap;word-break:break-all}
.pid{word-break:break-all;color:var(--dim);font-size:.8em;margin-top:4px}
.empty{color:var(--dim);font-style:italic}
</style>
</head>
<body>
<aside>
  <div class="logo">⬡ Metaverse<em id="logo-sub">node</em></div>
  <nav id="nav">
    <a class="active" onclick="go('overview',this)"><span class="ic">▦</span>Overview</a>
    <a onclick="go('peers',this)"><span class="ic">⇄</span>Peers</a>
    <a onclick="go('network',this)"><span class="ic">∿</span>Network</a>
    <a id="nav-world"   style="display:none" onclick="go('world',this)"><span class="ic">◈</span>World</a>
    <a id="nav-keys"    style="display:none" onclick="go('keys',this)"><span class="ic">⚿</span>Keys</a>
    <a id="nav-content" style="display:none" onclick="go('content',this)"><span class="ic">≡</span>Content</a>
    <a onclick="go('config',this)"><span class="ic">⚙</span>Config</a>
  </nav>
  <div class="node-meta">
    <div class="nm" id="m-name">—</div>
    <div><span class="nt nt-unknown" id="m-type">unknown</span></div>
    <div id="m-up"></div>
    <div id="m-ip"></div>
  </div>
</aside>
<main>
  <div class="topbar">
    <h2 id="page-title">Overview</h2>
    <span class="live ok" id="live-ind">● live</span>
  </div>
  <div id="content">Loading…</div>
</main>
<script>
const S={page:'overview',data:null,timer:null};
const TITLES={overview:'Overview',peers:'Peers',network:'Network',world:'World',keys:'Keys',content:'Content',config:'Config'};

function fmt(v){return v==null||v===undefined?'—':v}
function fmtB(b){b=b||0;if(b<1024)return b+' B';if(b<1048576)return (b/1024).toFixed(1)+' KB';if(b<1073741824)return (b/1048576).toFixed(1)+' MB';return (b/1073741824).toFixed(2)+' GB'}
function fmtUp(s){s=s||0;const h=Math.floor(s/3600),m=Math.floor((s%3600)/60),ss=s%60;return String(h).padStart(2,'0')+':'+String(m).padStart(2,'0')+':'+String(ss).padStart(2,'0')}
function bar(pct){const w=Math.min(100,Math.round(pct||0));const c=pct>80?'var(--red)':pct>60?'var(--yellow)':'var(--green)';return `<div class="bw"><div class="bf" style="width:${w}%;background:${c}"></div></div>`}
function badge(t){return `<span class="badge badge-${t||'unknown'}">${t||'unknown'}</span>`}
function short(id){return id&&id.length>18?'…'+id.slice(-14):id||'—'}

function go(p,el){
  S.page=p;
  document.querySelectorAll('nav a').forEach(a=>a.classList.remove('active'));
  if(el)el.classList.add('active');
  document.getElementById('page-title').textContent=TITLES[p]||p;
  if(S.data)render(p,S.data);
}

async function poll(){
  try{
    const r=await fetch('/api/status');
    if(!r.ok)throw new Error(r.status);
    S.data=await r.json();
    updateMeta(S.data);
    render(S.page,S.data);
    const li=document.getElementById('live-ind');
    li.textContent='● live';li.className='live ok';
  }catch(e){
    const li=document.getElementById('live-ind');
    li.textContent='● offline';li.className='live err';
  }
  S.timer=setTimeout(poll,5000);
}

function updateMeta(d){
  document.getElementById('m-name').textContent=d.node_name||'Unknown';
  document.getElementById('m-up').textContent='Up: '+fmtUp(d.uptime_secs);
  document.getElementById('m-ip').textContent=(d.public_ip||'')+':'+(d.p2p_port||'');
  document.getElementById('logo-sub').textContent=d.node_type||'node';
  document.title=(d.node_type||'Node')+' — '+(d.node_name||'');
  const mt=document.getElementById('m-type');
  mt.textContent=d.node_type||'unknown';
  mt.className='nt nt-'+(d.node_type||'unknown');
  const srv=d.node_type==='server';
  document.getElementById('nav-world').style.display=srv?'':'none';
  document.getElementById('nav-keys').style.display=srv?'':'none';
  document.getElementById('nav-content').style.display=srv?'':'none';
}

function render(p,d){
  const el=document.getElementById('content');
  if(p==='overview') el.innerHTML=pgOverview(d);
  else if(p==='peers')   el.innerHTML=pgPeers(d);
  else if(p==='network') el.innerHTML=pgNetwork(d);
  else if(p==='world')   el.innerHTML=pgWorld(d);
  else if(p==='config')  fetchPage('/api/config',el,raw=>pgConfig(raw));
  else if(p==='keys')    fetchPage('/api/keys',el,data=>pgKeys(data));
  else if(p==='content') fetchPage('/api/v1/content',el,data=>pgContent(data));
}

async function fetchPage(url,el,fn){
  el.innerHTML='<div class="sec"><span class="empty">Loading…</span></div>';
  try{const r=await fetch(url);if(!r.ok)throw new Error('HTTP '+r.status);el.innerHTML=fn(await r.json());}
  catch(e){el.innerHTML=`<div class="sec err">Failed: ${e.message}</div>`;}
}

function pgOverview(d){
  const ram=d.ram_total_mb>0?Math.round(d.ram_used_mb/d.ram_total_mb*100):0;
  const shed=d.shedding;
  const ex=d.extra||{};
  return `<div class="grid">
  <div class="card"><div class="card-title">Network</div>
    <div class="stat"><span class="lbl">Peers</span><span class="val">${(d.peers||[]).length}</span></div>
    <div class="stat"><span class="lbl">Circuits</span><span class="val">${fmt(d.circuit_count)}</span></div>
    <div class="stat"><span class="lbl">Total conns</span><span class="val">${fmt(d.total_connections)}</span></div>
    <div class="stat"><span class="lbl">DHT peers</span><span class="val">${fmt(d.dht_peer_count)}</span></div>
    <div class="stat"><span class="lbl">Status</span><span class="${shed?'warn':'ok'}">${shed?'⚠ Shedding':'✓ Normal'}</span></div>
  </div>
  <div class="card"><div class="card-title">Traffic</div>
    <div class="stat"><span class="lbl">Gossip in</span><span class="val">${fmt(d.gossip_msgs_in)}</span></div>
    <div class="stat"><span class="lbl">Gossip out</span><span class="val">${fmt(d.gossip_msgs_out)}</span></div>
    <div class="stat"><span class="lbl">Bytes in</span><span class="val">${fmtB(d.bytes_in)}</span></div>
    <div class="stat"><span class="lbl">Bytes out</span><span class="val">${fmtB(d.bytes_out)}</span></div>
  </div>
  <div class="card"><div class="card-title">System</div>
    <div class="stat"><span class="lbl">CPU</span><span class="val">${(d.cpu_pct||0).toFixed(1)}%</span></div>
    ${bar(d.cpu_pct)}
    <div class="stat" style="margin-top:7px"><span class="lbl">RAM</span><span class="val">${fmt(d.ram_used_mb)} / ${fmt(d.ram_total_mb)} MB</span></div>
    ${bar(ram)}
  </div>
  ${ex.world?`<div class="card"><div class="card-title">World</div>
    <div class="stat"><span class="lbl">Chunks loaded</span><span class="val">${ex.world.chunks_loaded||0}</span></div>
    <div class="stat"><span class="lbl">Voxel ops</span><span class="val">${ex.world.voxel_ops_total||0}</span></div>
    <div class="stat"><span class="lbl">Data</span><span class="val">${(ex.world.world_data_mb||0).toFixed(1)} MB</span></div>
    <div class="stat"><span class="lbl">Keys</span><span class="val">${ex.key_count||0}</span></div>
  </div>`:''}
  ${(ex.total_reservations!=null)?`<div class="card"><div class="card-title">Relay</div>
    <div class="stat"><span class="lbl">Reservations</span><span class="val">${ex.total_reservations}</span></div>
  </div>`:''}
</div>
<div class="sec"><div class="card-title">Identity</div>
  <div class="stat"><span class="lbl">Version</span><span class="val">${d.version||'—'}</span></div>
  <div class="stat"><span class="lbl">Web port</span><span class="val">${d.web_port||'—'}</span></div>
  <div class="pid">${d.peer_id||'—'}</div>
</div>`;
}

function pgPeers(d){
  const ps=d.peers||[];
  if(!ps.length)return '<div class="sec"><span class="empty">No peers connected.</span></div>';
  return `<div class="sec"><table><thead><tr><th>Peer ID</th><th>Type</th><th>Address</th><th>Connected</th></tr></thead><tbody>
${ps.map(p=>`<tr><td title="${p.peer_id}">${short(p.peer_id)}</td><td>${badge(p.peer_type)}</td><td>${p.addr||'—'}</td><td>${fmtUp(p.connected_secs)}</td></tr>`).join('')}
</tbody></table></div>`;
}

function pgNetwork(d){
  return `<div class="grid">
  <div class="card"><div class="card-title">Gossipsub</div>
    <div class="stat"><span class="lbl">Msgs in</span><span class="val">${fmt(d.gossip_msgs_in)}</span></div>
    <div class="stat"><span class="lbl">Msgs out</span><span class="val">${fmt(d.gossip_msgs_out)}</span></div>
  </div>
  <div class="card"><div class="card-title">Bandwidth</div>
    <div class="stat"><span class="lbl">Total in</span><span class="val">${fmtB(d.bytes_in)}</span></div>
    <div class="stat"><span class="lbl">Total out</span><span class="val">${fmtB(d.bytes_out)}</span></div>
  </div>
  <div class="card"><div class="card-title">DHT / Relay</div>
    <div class="stat"><span class="lbl">DHT peers</span><span class="val">${fmt(d.dht_peer_count)}</span></div>
    <div class="stat"><span class="lbl">Circuits active</span><span class="val">${fmt(d.circuit_count)}</span></div>
    <div class="stat"><span class="lbl">Total conns</span><span class="val">${fmt(d.total_connections)}</span></div>
  </div>
</div>`;
}

function pgWorld(d){
  const w=(d.extra||{}).world;
  if(!w)return '<div class="sec"><span class="empty">No world data available.</span></div>';
  return `<div class="grid">
  <div class="card"><div class="card-title">Chunks</div>
    <div class="stat"><span class="lbl">Loaded</span><span class="val">${w.chunks_loaded||0}</span></div>
    <div class="stat"><span class="lbl">Queued</span><span class="val">${w.chunks_queued||0}</span></div>
  </div>
  <div class="card"><div class="card-title">Operations</div>
    <div class="stat"><span class="lbl">Voxel ops</span><span class="val">${w.voxel_ops_total||0}</span></div>
    <div class="stat"><span class="lbl">Merged</span><span class="val">${w.ops_merged_total||0}</span></div>
  </div>
  <div class="card"><div class="card-title">Storage</div>
    <div class="stat"><span class="lbl">Size</span><span class="val">${(w.world_data_mb||0).toFixed(2)} MB</span></div>
    <div class="stat"><span class="lbl">Last save</span><span class="val">${w.last_save_secs_ago<3600?w.last_save_secs_ago+'s ago':'n/a'}</span></div>
    <div class="stat"><span class="lbl">Shedding</span><span class="${w.shedding_chunks?'warn':'ok'}">${w.shedding_chunks?'⚠ Yes':'✓ No'}</span></div>
  </div>
</div>`;
}

function pgKeys(data){
  const ks=Array.isArray(data)?data:(data.keys||[]);
  if(!ks.length)return '<div class="sec"><span class="empty">No registered keys.</span></div>';
  return `<div class="sec"><table><thead><tr><th>Peer ID</th><th>Type</th><th>Name</th><th>Issued</th><th>Status</th></tr></thead><tbody>
${ks.map(k=>`<tr>
  <td title="${k.peer_id||''}">${short(k.peer_id||k.public_key_hex||'')}</td>
  <td>${badge(k.key_type)}</td>
  <td>${k.display_name||'—'}</td>
  <td>${k.issued_at?new Date(k.issued_at).toLocaleDateString():'—'}</td>
  <td>${k.revoked?'<span class="err">Revoked</span>':'<span class="ok">Active</span>'}</td>
</tr>`).join('')}
</tbody></table></div>`;
}

function pgContent(data){
  const items=Array.isArray(data)?data:(data.items||[]);
  if(!items.length)return '<div class="sec"><span class="empty">No content items.</span></div>';
  return `<div class="sec"><table><thead><tr><th>Section</th><th>Title</th><th>Author</th><th>Time</th></tr></thead><tbody>
${items.map(it=>`<tr>
  <td>${it.section||'—'}</td>
  <td>${(it.title||'').slice(0,55)}</td>
  <td>${it.author_display||it.author||'—'}</td>
  <td>${it.created_at?new Date(it.created_at).toLocaleString():'—'}</td>
</tr>`).join('')}
</tbody></table></div>`;
}

function pgConfig(cfg){
  return `<div class="sec"><div class="card-title">Current Configuration</div><pre>${JSON.stringify(cfg,null,2)}</pre></div>`;
}

poll();
</script>
</body>
</html>"#;
