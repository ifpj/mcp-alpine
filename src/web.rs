use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use axum::{routing, Router};
use serde::Deserialize;

use crate::log::LogStore;

#[derive(Deserialize)]
pub struct LogsQuery {
    since: Option<u64>,
}

async fn api_logs(
    State(store): State<Arc<LogStore>>,
    Query(q): Query<LogsQuery>,
) -> impl IntoResponse {
    let entries = match q.since {
        Some(id) => store.since(id),
        None => store.all(),
    };
    axum::Json(entries)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub fn web_routes(log_store: Arc<LogStore>) -> Router {
    Router::new()
        .route("/", routing::get(index))
        .route("/api/logs", routing::get(api_logs))
        .with_state(log_store)
}

const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>mcp-alpine Logs</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{background:#1a1b26;color:#c0caf5;font-family:'SF Mono','Fira Code',Consolas,monospace;padding:20px;min-height:100vh}
h1{font-size:1.4em;color:#7aa2f7;margin-bottom:16px}
.status{color:#565f89;font-size:.85em;margin-bottom:16px}
.entry{background:#24283b;border-radius:8px;padding:16px;margin-bottom:12px;border-left:3px solid #9ece6a;transition:border-color .2s}
.entry.err{border-left-color:#f7768e}
.hd{display:flex;justify-content:space-between;align-items:center;margin-bottom:10px;gap:8px;flex-wrap:wrap}
.cmd{color:#7aa2f7;font-size:.95em;flex:1;word-break:break-all}
.tm{color:#565f89;font-size:.75em;white-space:nowrap}
.badge{display:inline-block;padding:1px 6px;border-radius:4px;font-size:.72em;font-weight:bold;margin-left:6px}
.badge.ok{background:#1a3a2a;color:#9ece6a}
.badge.bad{background:#3a1a1a;color:#f7768e}
pre{background:#1e2030;padding:8px 12px;border-radius:6px;margin-top:8px;overflow-x:auto;font-size:.83em;line-height:1.55;white-space:pre-wrap;word-break:break-all;max-height:400px;overflow-y:auto}
.so{color:#a9b1d6}
.se{color:#f7768e}
.empty{color:#565f89;text-align:center;padding:48px 0;font-size:1.05em}
</style>
</head>
<body>
<h1>mcp-alpine Logs</h1>
<div class="status">Live &middot; polling every 2s &middot; <a id="clearBtn" href="#" style="color:#565f89">clear</a></div>
<div id="logs"><div class="empty">No entries yet</div></div>
<script>
const logsEl=document.getElementById('logs');
const clearBtn=document.getElementById('clearBtn');
let lastId=0,firstLoad=true;
function esc(s){const d=document.createElement('div');d.textContent=s;return d.innerHTML}
function render(e){
  const ok=e.exit_code===0;
  let h='<div class="hd"><div><span class="cmd">$ '+esc(e.command)+'</span>'
    +'<span class="badge '+(ok?'ok':'bad')+'">exit '+e.exit_code+'</span>'
    +'<span class="badge ok">'+e.duration_ms+'ms</span></div>'
    +'<span class="tm">'+esc(e.time)+'</span></div>';
  const so=e.stdout,se=e.stderr;
  if(so||se){
    h+='<pre>';
    if(so)h+='<span class="so">'+esc(so)+'</span>';
    if(se){if(so)h+='\n';h+='<span class="se">[stderr]\n'+esc(se)+'</span>';}
    h+='</pre>';
  }
  const div=document.createElement('div');
  div.className='entry'+(ok?'':' err');
  div.innerHTML=h;
  return div;
}
async function fetchLogs(){
  try{
    const r=await fetch('/api/logs?since='+lastId);
    const data=await r.json();
    if(data.length===0)return;
    const empty=logsEl.querySelector('.empty');
    if(empty)empty.remove();
    data.forEach(e=>{logsEl.appendChild(render(e));lastId=e.id;});
    if(firstLoad&&data.length>0){window.scrollTo(0,document.body.scrollHeight);firstLoad=false;}
    else window.scrollTo(0,document.body.scrollHeight);
  }catch(err){console.error(err)}
}
fetchLogs();
setInterval(fetchLogs,2000);
clearBtn.onclick=ev=>{ev.preventDefault();if(confirm('Clear displayed logs?')){logsEl.innerHTML='<div class="empty">No entries yet</div>';lastId=0;}};
</script>
</body>
</html>
"##;
