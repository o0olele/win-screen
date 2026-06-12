import{i as g,t as D}from"./core-B8jQGe47.js";var y;(function(i){i.WINDOW_RESIZED="tauri://resize",i.WINDOW_MOVED="tauri://move",i.WINDOW_CLOSE_REQUESTED="tauri://close-requested",i.WINDOW_DESTROYED="tauri://destroyed",i.WINDOW_FOCUS="tauri://focus",i.WINDOW_BLUR="tauri://blur",i.WINDOW_SCALE_FACTOR_CHANGED="tauri://scale-change",i.WINDOW_THEME_CHANGED="tauri://theme-changed",i.WINDOW_CREATED="tauri://window-created",i.WINDOW_SUSPENDED="tauri://suspended",i.WINDOW_RESUMED="tauri://resumed",i.WEBVIEW_CREATED="tauri://webview-created",i.DRAG_ENTER="tauri://drag-enter",i.DRAG_OVER="tauri://drag-over",i.DRAG_DROP="tauri://drag-drop",i.DRAG_LEAVE="tauri://drag-leave"})(y||(y={}));async function $(i,t){window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(i,t),await g("plugin:event|unlisten",{event:i,eventId:t})}async function w(i,t,c){var n;const l=(n=void 0)!==null&&n!==void 0?n:{kind:"Any"};return g("plugin:event|listen",{event:i,target:l,handler:D(t)}).then(m=>async()=>$(i,m))}const _=document.querySelector("#app");if(!_)throw new Error("missing #app");const h=_;let a=null,p=[],u=!1,e="Ready";function s(i,t){return g(i,t)}async function d(i){u=!0,o();try{await i()}catch(t){e=t instanceof Error?t.message:String(t)}finally{u=!1,o()}}async function v(){await d(async()=>{e="Selecting region...",o(),await s("start_interactive_capture_flow",{inlineBase64:!0}),e="Select a region, then use the floating toolbar"})}async function W(){if(!(a!=null&&a.base64Png)){e="Select a region first",o();return}await d(async()=>{e=`Created pin ${(await s("pin_image",{options:{base64Image:a==null?void 0:a.base64Png}})).id}`,await r()})}async function N(){if(!(a!=null&&a.base64Png)){e="Select a region first",o();return}await d(async()=>{const i=await s("pin_image",{options:{base64Image:a==null?void 0:a.base64Png}});await s("copy_pin",{id:i.id}),await s("close_pin",{id:i.id}),e="Copied selected image via temporary pin",await r()})}async function r(){p=await s("list_pins")}async function f(i,t){await d(async()=>{await s("set_pin_opacity",{options:{id:i,opacity:t}}),e=`Set pin ${i} opacity to ${Math.round(t*100)}%`,await r()})}async function E(i){await d(async()=>{await s("close_pin",{id:i}),e=`Closed pin ${i}`,await r()})}async function b(){await d(async()=>{await r(),e=`Loaded ${p.length} pin${p.length===1?"":"s"}`})}function R(){return a!=null&&a.base64Png?`
    <div class="preview-toolbar">
      <button data-action="pin-selection">Pin</button>
      <button data-action="copy-selection">Copy</button>
      <button data-action="clear-selection">Clear</button>
    </div>
    <img class="preview-image" src="data:image/png;base64,${a.base64Png}" alt="Selected capture" />
    <div class="meta">
      ${a.width}x${a.height}
      · screen ${a.rect.x}, ${a.rect.y}
    </div>
  `:'<div class="empty">No selection</div>'}function I(){return p.length===0?'<div class="empty compact">No active pins</div>':p.map(i=>{const t=i.displaySize??i.display_size??i.size??{width:0,height:0},c=i.position??{x:0,y:0};return`
        <div class="pin-row">
          <div>
            <strong>#${i.id}</strong>
            <span>${t.width}x${t.height}</span>
            <span>${c.x}, ${c.y}</span>
          </div>
          <div class="pin-actions">
            <button data-action="opacity-100" data-id="${i.id}">100%</button>
            <button data-action="opacity-70" data-id="${i.id}">70%</button>
            <button data-action="close-pin" data-id="${i.id}">Close</button>
          </div>
        </div>
      `}).join("")}function o(){h.innerHTML=`
    <main>
      <header>
        <div>
          <h1>win-screen Tauri Demo</h1>
          <p>Native overlay selection with a floating WebView toolbar.</p>
        </div>
        <button data-action="select-region" ${u?"disabled":""}>Select Region</button>
      </header>

      <section class="grid">
        <article class="panel preview">
          <h2>Selection</h2>
          ${R()}
        </article>

        <article class="panel">
          <div class="panel-heading">
            <h2>Pins</h2>
            <button data-action="refresh-pins" ${u?"disabled":""}>Refresh</button>
          </div>
          ${I()}
        </article>
      </section>

      <footer>${e}</footer>
    </main>
  `}h.addEventListener("click",i=>{const c=i.target.closest("button[data-action]");if(!c||u)return;const n=c.dataset.action,l=Number(c.dataset.id);n==="select-region"&&v(),n==="pin-selection"&&W(),n==="copy-selection"&&N(),n==="clear-selection"&&(a=null,e="Selection cleared",o()),n==="refresh-pins"&&b(),n==="opacity-100"&&f(l,1),n==="opacity-70"&&f(l,.7),n==="close-pin"&&E(l)});b();w("win-screen-demo://selection-done",async i=>{a=i.payload;const t=i.payload.pinned&&i.payload.pinId?`, pinned #${i.payload.pinId}`:"";e=`Selected ${i.payload.width}x${i.payload.height}${t}`,await r(),o()});w("win-screen-demo://selection-canceled",()=>{e="Selection canceled",o()});o();
