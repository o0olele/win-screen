import{i as I,t as M}from"./core-B8jQGe47.js";var L;(function(t){t.WINDOW_RESIZED="tauri://resize",t.WINDOW_MOVED="tauri://move",t.WINDOW_CLOSE_REQUESTED="tauri://close-requested",t.WINDOW_DESTROYED="tauri://destroyed",t.WINDOW_FOCUS="tauri://focus",t.WINDOW_BLUR="tauri://blur",t.WINDOW_SCALE_FACTOR_CHANGED="tauri://scale-change",t.WINDOW_THEME_CHANGED="tauri://theme-changed",t.WINDOW_CREATED="tauri://window-created",t.WINDOW_SUSPENDED="tauri://suspended",t.WINDOW_RESUMED="tauri://resumed",t.WEBVIEW_CREATED="tauri://webview-created",t.DRAG_ENTER="tauri://drag-enter",t.DRAG_OVER="tauri://drag-over",t.DRAG_DROP="tauri://drag-drop",t.DRAG_LEAVE="tauri://drag-leave"})(L||(L={}));async function B(t,e){window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(t,e),await I("plugin:event|unlisten",{event:t,eventId:e})}async function R(t,e,o){var i;const d=(i=void 0)!==null&&i!==void 0?i:{kind:"Any"};return I("plugin:event|listen",{event:t,target:d,handler:M(e)}).then(h=>async()=>B(t,h))}const z=document.querySelector("#app");let v="capture",n=null,$=[],m=[],r=!1,a="Ready",b=null,E=0,f=null,_="",P=!0,W=!1,l="fullscreen",w=null,p=null;function c(t,e){return I(t,e)}async function u(t){r=!0,s();try{await t()}catch(e){a=e instanceof Error?e.message:String(e)}finally{r=!1,s()}}function g(t,e){return typeof t=="number"&&Number.isFinite(t)?t:e}function H(t){var i,d,h,N,O,S,C,A;if(!t||typeof t.id!="number")return null;const e={width:g((i=t.size)==null?void 0:i.width,0),height:g((d=t.size)==null?void 0:d.height,0)},o={width:g((h=t.displaySize??t.display_size??t.size)==null?void 0:h.width,e.width),height:g((N=t.displaySize??t.display_size??t.size)==null?void 0:N.height,e.height)};return{id:t.id,size:e,displaySize:o,position:{x:g((O=t.position)==null?void 0:O.x,0),y:g((S=t.position)==null?void 0:S.y,0),width:g((C=t.position)==null?void 0:C.width,0),height:g((A=t.position)==null?void 0:A.height,0)},opacity:g(t.opacity,1)}}function U(t){const e=Math.floor(t/60).toString().padStart(2,"0"),o=(t%60).toString().padStart(2,"0");return`${e}:${o}`}function D(t){return t.replace(/&/g,"&amp;").replace(/"/g,"&quot;").replace(/</g,"&lt;")}async function y(){$=(await c("list_pins")).map(H).filter(e=>e!==null)}async function x(){await u(async()=>{await y(),a=`${$.length} pin${$.length===1?"":"s"} loaded`})}async function G(){try{m=await c("list_monitors_demo")}catch{m=[]}}async function V(){await u(async()=>{a="Selecting region…",s(),await c("start_interactive_capture_flow",{inlineBase64:!0}),a="Select a region, then use the floating toolbar"})}async function j(){await u(async()=>{a="Capturing fullscreen…",s();const t=await c("capture_fullscreen_demo",{clipboard:!1,inlineBase64:!0});n={rect:{x:0,y:0,width:t.width,height:t.height},width:t.width,height:t.height,base64Png:t.base64Png,pinned:!1},a=`Captured fullscreen ${t.width}×${t.height}`})}async function q(t){await u(async()=>{a=`Capturing monitor ${t}…`,s();const e=await c("capture_monitor_demo",{monitor:t,clipboard:!1,inlineBase64:!0});n={rect:{x:0,y:0,width:e.width,height:e.height},width:e.width,height:e.height,base64Png:e.base64Png,pinned:!1},a=`Captured monitor ${t}: ${e.width}×${e.height}`})}async function F(){if(!(n!=null&&n.base64Png)){a="No selection to pin",s();return}await u(async()=>{a=`Created pin ${(await c("pin_image",{options:{base64Image:n==null?void 0:n.base64Png}})).id}`,await y()})}async function T(){if(!(n!=null&&n.base64Png)){a="No selection to copy",s();return}await u(async()=>{const t=await c("pin_image",{options:{base64Image:n==null?void 0:n.base64Png}});await c("copy_pin",{id:t.id}),await c("close_pin",{id:t.id}),a="Copied to clipboard",await y()})}async function Q(){await u(async()=>{a="框选录制区域，ESC 取消…",s();const t=await c("select_record_region");t?(p=t,l="region",a=`已选定区域：${t[2]}×${t[3]}，起点 (${t[0]}, ${t[1]})`):a="区域选择已取消"})}async function Y(){if(b===null){if(!_.trim()){a="请先填写输出路径",s();return}if(l==="region"&&!p){a="请先选定录制区域",s();return}await u(async()=>{const t=await c("start_recording_demo",{output:_.trim(),systemAudio:P,microphone:W,monitor:l==="monitor"?w??void 0:void 0,region:l==="region"?p:void 0});b=t,E=0,f=setInterval(()=>{E++,s()},1e3),l==="region"&&p&&c("show_region_indicator",{rect:p}).catch(()=>{}),a=`录屏已开始 (id ${t})`})}}async function Z(){if(b===null)return;const t=b;await u(async()=>{f!==null&&(clearInterval(f),f=null);const e=await c("stop_recording_demo",{id:t});b=null,a=`Saved: ${e}`}),c("hide_region_indicator").catch(()=>{})}async function k(t,e){await u(async()=>{await c("set_pin_opacity",{options:{id:t,opacity:e}}),a=`Pin ${t} opacity → ${Math.round(e*100)}%`,await y()})}async function J(t){await u(async()=>{await c("copy_pin",{id:t}),a=`Pin ${t} copied to clipboard`})}async function K(t){await u(async()=>{await c("close_pin",{id:t}),a=`Pin ${t} closed`,await y()})}function X(){return`<nav class="tabs">${[{id:"capture",label:"截图"},{id:"record",label:"录屏"},{id:"pins",label:`贴图${$.length?` (${$.length})`:""}`}].map(e=>`<button class="tab-btn${v===e.id?" active":""}" data-action="switch-tab" data-tab="${e.id}">${e.label}</button>`).join("")}</nav>`}function tt(){const t=n!=null&&n.base64Png?`<div class="preview-toolbar">
        <button data-action="pin-selection" ${r?"disabled":""}>贴到桌面</button>
        <button data-action="copy-selection" ${r?"disabled":""}>复制</button>
        <button data-action="clear-selection">清除</button>
       </div>
       <img class="preview-image" src="data:image/png;base64,${n.base64Png}" alt="capture" />
       <div class="meta">${n.width}×${n.height} · (${n.rect.x}, ${n.rect.y})</div>`:'<div class="empty">暂无截图</div>',e=m.length>0?m.map(o=>`<div class="monitor-row">
              <div>
                <span class="monitor-label">${o.primary?`显示器 ${o.id}（主屏）`:`显示器 ${o.id}`}</span>
                <span class="meta">${o.rect.width}×${o.rect.height}</span>
              </div>
              <button data-action="capture-monitor" data-id="${o.id}" ${r?"disabled":""}>截图</button>
            </div>`).join(""):'<div class="meta" style="padding:8px 0">未检测到显示器</div>';return`<div class="capture-grid">
    <section class="panel preview-panel">
      <div class="panel-heading">
        <h2>预览</h2>
        <button data-action="select-region" ${r?"disabled":""}>框选区域</button>
      </div>
      ${t}
    </section>
    <section class="panel direct-panel">
      <h2>直接截图</h2>
      <button class="full-btn" data-action="capture-fullscreen" ${r?"disabled":""}>全屏截图</button>
      <div class="monitor-list">${e}</div>
    </section>
  </div>`}function et(){const t=b!==null,e=m.map(i=>`<option value="${i.id}" ${w===i.id?"selected":""}>${i.primary?`显示器 ${i.id}（主屏）`:`显示器 ${i.id}`} ${i.rect.width}×${i.rect.height}</option>`).join(""),o=p?`${p[2]}×${p[3]}，起点 (${p[0]}, ${p[1]})`:"未选定";return`<div class="record-form">
    <label class="form-row">
      <span class="form-label">输出路径</span>
      <input class="form-input" type="text" id="record-output"
        placeholder="例：C:\\Users\\你的名字\\Desktop\\recording.mp4"
        value="${D(_)}" ${t?"disabled":""} />
    </label>

    <div class="form-row">
      <span class="form-label">录制目标</span>
      <span class="radio-group">
        <label><input type="radio" name="rec-target" value="fullscreen" ${l==="fullscreen"?"checked":""} ${t?"disabled":""} /> 全屏</label>
        <label><input type="radio" name="rec-target" value="monitor" ${l==="monitor"?"checked":""} ${t?"disabled":""} /> 指定显示器</label>
        <select id="rec-monitor" ${l!=="monitor"||t?"disabled":""}>${e||'<option value="">—</option>'}</select>
        <label><input type="radio" name="rec-target" value="region" ${l==="region"?"checked":""} ${t?"disabled":""} /> 选定区域</label>
        <button data-action="select-record-region" ${t||r?"disabled":""}>框选区域</button>
        ${l==="region"?`<span class="region-hint">${D(o)}</span>`:""}
      </span>
    </div>

    <div class="form-row">
      <span class="form-label">音频</span>
      <span class="checkbox-group">
        <label><input type="checkbox" id="audio-system" ${P?"checked":""} ${t?"disabled":""} /> 系统声音</label>
        <label><input type="checkbox" id="audio-mic" ${W?"checked":""} ${t?"disabled":""} /> 麦克风</label>
      </span>
    </div>

    <div class="record-controls">
      <button class="record-btn${t?" recording":""}"
        data-action="${t?"stop-recording":"start-recording"}"
        ${r?"disabled":""}>${t?"■ 停止录屏":"▶ 开始录屏"}</button>
      ${t?`<span class="timer">${U(E)}</span>`:""}
    </div>

    ${t?'<div class="record-status">正在录屏中…</div>':""}
  </div>`}function it(){return $.length===0?'<div class="empty compact">暂无贴图</div>':$.map(t=>`<div class="pin-row">
      <div class="pin-info">
        <strong>#${t.id}</strong>
        <span>${t.displaySize.width}×${t.displaySize.height}</span>
        <span>${t.position.x}, ${t.position.y}</span>
        <span>${Math.round(t.opacity*100)}%</span>
      </div>
      <div class="pin-actions">
        <button data-action="opacity-100" data-id="${t.id}" ${r?"disabled":""}>100%</button>
        <button data-action="opacity-70" data-id="${t.id}" ${r?"disabled":""}>70%</button>
        <button data-action="copy-pin" data-id="${t.id}" ${r?"disabled":""}>复制</button>
        <button data-action="close-pin" data-id="${t.id}" ${r?"disabled":""}>关闭</button>
      </div>
    </div>`).join("")}function s(){let t;v==="capture"?t=tt():v==="record"?t=et():t=`<section class="panel pins-panel">
      <div class="panel-heading">
        <h2>贴图列表</h2>
        <button data-action="refresh-pins" ${r?"disabled":""}>刷新</button>
      </div>
      ${it()}
    </section>`,z.innerHTML=`<main>
    <header>
      <div><h1>win-screen Demo</h1><p>Windows 截图 · 录屏 · 桌面贴图</p></div>
      ${X()}
    </header>
    <div class="tab-content">${t}</div>
    <footer class="status">${D(a)}</footer>
  </main>`;const e=document.getElementById("record-output");e==null||e.addEventListener("input",()=>{_=e.value});const o=document.getElementById("audio-system");o==null||o.addEventListener("change",()=>{P=o.checked});const i=document.getElementById("audio-mic");i==null||i.addEventListener("change",()=>{W=i.checked}),document.querySelectorAll('input[name="rec-target"]').forEach(h=>{h.addEventListener("change",()=>{l=h.value,l==="monitor"&&m.length>0&&w===null&&(w=m[0].id),s()})});const d=document.getElementById("rec-monitor");d==null||d.addEventListener("change",()=>{w=Number(d.value)})}z.addEventListener("click",t=>{const o=t.target.closest("button[data-action]");if(!o)return;const i=o.dataset.action,d=Number(o.dataset.id);if(i==="switch-tab"){v=o.dataset.tab??"capture",s();return}r||(i==="select-region"?V():i==="capture-fullscreen"?j():i==="capture-monitor"?q(d):i==="pin-selection"?F():i==="copy-selection"?T():i==="clear-selection"?(n=null,a="Cleared",s()):i==="refresh-pins"?x():i==="opacity-100"?k(d,1):i==="opacity-70"?k(d,.7):i==="copy-pin"?J(d):i==="close-pin"?K(d):i==="select-record-region"?Q():i==="start-recording"?Y():i==="stop-recording"&&Z())});R("win-screen-demo://selection-done",async t=>{n=t.payload;const e=t.payload.pinned&&t.payload.pinId?`, pinned #${t.payload.pinId}`:"";a=`Selected ${t.payload.width}×${t.payload.height}${e}`,await y(),s()});R("win-screen-demo://selection-canceled",()=>{a="Selection canceled",s()});R("win-screen-demo://recording-stopped",t=>{f!==null&&(clearInterval(f),f=null),b=null,c("hide_region_indicator").catch(()=>{}),a=`Recording saved: ${t.payload}`,s()});Promise.all([x(),G()]).then(()=>s());s();
