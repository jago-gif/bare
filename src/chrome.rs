pub const CHROME_HTML: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
html,body{margin:0;height:100%;background:#1e1f22;color:#e6e6e6;font:13px 'Segoe UI',system-ui,sans-serif;overflow:hidden;user-select:none}
#bar{display:flex;align-items:center;height:44px;padding:0 10px;gap:6px}
#tabs{display:flex;align-items:center;gap:4px;min-width:0;flex:1;height:100%}
.tab{display:flex;align-items:center;gap:6px;padding:0 10px;height:30px;border-radius:8px;background:#2b2d31;max-width:220px;min-width:0;white-space:nowrap}
.tab.active{background:#3d3f45}
.tab .title{flex:1;overflow:hidden;text-overflow:ellipsis;min-width:0}
.tab .close{width:16px;text-align:center;border-radius:4px;opacity:.7}
.tab .close:hover{background:#5a5a5a;opacity:1}
#newtab{width:28px;height:30px;border-radius:6px;background:#2b2d31;display:flex;align-items:center;justify-content:center}
#tools{display:flex;align-items:center;gap:4px;margin-left:6px}
button{background:#2b2d31;color:#e6e6e6;border:0;width:30px;height:30px;border-radius:6px;font-size:14px}
button:disabled{opacity:.35}
#omni{flex:1;margin-left:4px;height:30px;border-radius:15px;border:0;padding:0 12px;background:#15161a;color:#e6e6e6;font-size:13px}
#omni:focus{outline:1px solid #4c8bf5}
#shield{padding:0 10px;height:30px;border-radius:15px;background:#15161a;display:none;align-items:center;font-size:12px;color:#79c76f;white-space:nowrap}
#star{color:#e6e6e6}
#star.on{color:#f5c451}
#panel{position:absolute;top:44px;left:0;right:0;bottom:0;background:#26272b;padding:12px 16px;display:none;flex-direction:column;z-index:20;overflow:auto}
#panel .sec{margin-bottom:12px}
#panel .sec:last-child{margin-bottom:0}
#panel .head{display:flex;align-items:center;gap:8px;margin-bottom:4px}
#panel .head b{font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:#9a9da6}
#panel .head .flex{flex:1}
#panel .closebtn{font-size:12px;color:#9a9da6;cursor:pointer;padding:2px 6px;border-radius:4px}
#panel .closebtn:hover{background:#33353a;color:#e6e6e6}
#panel .clear{font-size:11px;color:#9a9da6;cursor:pointer;padding:2px 6px;border-radius:4px}
#panel .clear:hover{background:#33353a;color:#e6e6e6}
#panel .item{display:flex;gap:8px;align-items:baseline;padding:3px 6px;border-radius:6px;cursor:pointer}
#panel .item:hover{background:#33353a}
#panel .item .t{flex:0 0 auto;max-width:48%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:13px}
#panel .item .u{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#9a9da6;font-size:12px}
#panel .item .rm{visibility:hidden;color:#a8848e;cursor:pointer;font-size:12px}
#panel .item:hover .rm{visibility:visible}
#panel .empty{color:#6f7278;font-size:12px;padding:4px 6px}
</style>
</head>
<body>
<div id="bar">
  <div id="tabs"></div>
  <div id="newtab" onclick="post({cmd:'new-tab'})">+</div>
  <div id="tools">
    <button id="back" onclick="post({cmd:'back'})">&#8592;</button>
    <button id="fwd" onclick="post({cmd:'forward'})">&#8594;</button>
    <button id="reload" onclick="post({cmd:'reload'})">&#10227;</button>
    <input id="omni" spellcheck="false">
    <div id="shield"></div>
    <button id="star" onclick="post({cmd:'toggle-bookmark'})">&#9733;</button>
    <button id="panelbtn" onclick="post({cmd:'open-panel'})">&#9776;</button>
  </div>
</div>
<div id="panel">
  <div class="head">
    <b>Historial y marcadores</b>
    <span class="flex"></span>
    <span class="closebtn" onclick="post({cmd:'close-panel'})">&#10005; cerrar</span>
  </div>
  <div class="sec">
    <div class="head"><b>Marcadores</b></div>
    <div id="p-bookmarks"></div>
  </div>
  <div class="sec">
    <div class="head"><b>Historial</b><span class="flex"></span><span class="clear" onclick="post({cmd:'clear-history'})">limpiar</span></div>
    <div id="p-history"></div>
  </div>
</div>
<script>
function post(m){ ipc.postMessage(JSON.stringify(m)); }
const tabsEl=document.getElementById('tabs'),omni=document.getElementById('omni'),
      back=document.getElementById('back'),fwd=document.getElementById('fwd'),
      shield=document.getElementById('shield'),star=document.getElementById('star'),
      panel=document.getElementById('panel');
function updateChrome(tabs){
  tabsEl.innerHTML='';
  let cur=null;
  tabs.forEach(t=>{
    const d=document.createElement('div');
    d.className='tab'+(t.active?' active':'');
    d.innerHTML='<span class="title"></span><span class="close">&#10005;</span>';
    d.querySelector('.title').textContent=t.title||'Nueva pesta\u00f1a';
    d.onclick=()=>post({cmd:'select-tab',id:t.id});
    d.querySelector('.close').onclick=e=>{e.stopPropagation();post({cmd:'close-tab',id:t.id});};
    tabsEl.appendChild(d);
    if(t.active)cur=t;
  });
  if(cur){
    if(document.activeElement!==omni)omni.value=cur.url||'';
    back.disabled=!cur.canBack;
    fwd.disabled=!cur.canForward;
    shield.textContent=cur.blocked>0?('Bloqueados: '+cur.blocked):'';
    shield.style.display=cur.blocked>0?'flex':'none';
    star.classList.toggle('on',!!cur.bookmarked);
  }
}
function setPanel(open){ panel.style.display=open?'block':'none'; }
function updatePanel(data){
  renderList(document.getElementById('p-bookmarks'),data.bookmarks,'bookmark');
  renderList(document.getElementById('p-history'),data.history,'history');
}
function renderList(el,items,kind){
  el.innerHTML='';
  if(!items||!items.length){el.innerHTML='<div class="empty">Sin entradas</div>';return;}
  items.forEach(it=>{
    const d=document.createElement('div');
    d.className='item';
    d.innerHTML='<span class="t"></span><span class="u"></span>'+(kind==='bookmark'?'<span class="rm">&#10005;</span>':'');
    d.querySelector('.t').textContent=it.title||'(sin titulo)';
    d.querySelector('.u').textContent=(it.time?it.time+' \u00b7 ':'')+it.url;
    d.onclick=()=>post({cmd:'navigate',input:it.url});
    const rm=d.querySelector('.rm');
    if(rm)rm.onclick=e=>{e.stopPropagation();post({cmd:'remove-bookmark',url:it.url});};
    el.appendChild(d);
  });
}
omni.addEventListener('keydown',e=>{ if(e.key==='Enter'){ post({cmd:'navigate',input:omni.value}); omni.blur(); }});
omni.addEventListener('focus',()=>omni.select());
window.addEventListener('keydown',e=>{
  if(e.ctrlKey&&!e.shiftKey&&!e.altKey){
    const k=e.key.toLowerCase();
    if(k==='t'){e.preventDefault();post({cmd:'new-tab'});}
    else if(k==='w'){e.preventDefault();post({cmd:'close-active-tab'});}
    else if(k==='l'){e.preventDefault();omni.focus();}
    else if(k==='r'&&document.activeElement!==omni){e.preventDefault();post({cmd:'reload'});}
  }
});
</script>
</body>
</html>
"#;

const LOGO_B64: &str = include_str!("../assets/logo.b64");

pub fn home_html() -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
html,body{{margin:0;height:100%;display:flex;align-items:center;justify-content:center;background:#050505;color:#7d8087;font:15px 'Segoe UI',system-ui,sans-serif}}
img{{width:520px;max-width:70vw}}
p{{margin-top:4px;letter-spacing:.15em}}
</style>
</head>
<body>
<div style="text-align:center">
  <img src="data:image/png;base64,{LOGO_B64}" alt="Bare">
  <p>NOTHING EXTRA. JUST BROWSING.</p>
</div>
</body>
</html>
"#
    )
}
