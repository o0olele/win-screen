import{i as f,t as W}from"./core-B8jQGe47.js";var h;(function(i){i.WINDOW_RESIZED="tauri://resize",i.WINDOW_MOVED="tauri://move",i.WINDOW_CLOSE_REQUESTED="tauri://close-requested",i.WINDOW_DESTROYED="tauri://destroyed",i.WINDOW_FOCUS="tauri://focus",i.WINDOW_BLUR="tauri://blur",i.WINDOW_SCALE_FACTOR_CHANGED="tauri://scale-change",i.WINDOW_THEME_CHANGED="tauri://theme-changed",i.WINDOW_CREATED="tauri://window-created",i.WINDOW_SUSPENDED="tauri://suspended",i.WINDOW_RESUMED="tauri://resumed",i.WEBVIEW_CREATED="tauri://webview-created",i.DRAG_ENTER="tauri://drag-enter",i.DRAG_OVER="tauri://drag-over",i.DRAG_DROP="tauri://drag-drop",i.DRAG_LEAVE="tauri://drag-leave"})(h||(h={}));async function N(i,t){window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(i,t),await f("plugin:event|unlisten",{event:i,eventId:t})}async function b(i,t,d){var e;const l=(e=void 0)!==null&&e!==void 0?e:{kind:"Any"};return f("plugin:event|listen",{event:i,target:l,handler:W(t)}).then($=>async()=>N(i,$))}const _=document.querySelector("#app");if(!_)throw new Error("missing #app");const m=_;let n=null,g=[],y=!1,a="Ready";function s(i,t){return f(i,t)}async function r(i){y=!0,o();try{await i()}catch(t){a=t instanceof Error?t.message:String(t)}finally{y=!1,o()}}async function R(){await r(async()=>{a="Selecting region...",o(),await s("start_interactive_capture_flow",{inlineBase64:!0}),a="Select a region, then use the floating toolbar"})}async function E(){if(!(n!=null&&n.base64Png)){a="Select a region first",o();return}await r(async()=>{a=`Created pin ${(await s("pin_image",{options:{base64Image:n==null?void 0:n.base64Png}})).id}`,await p()})}async function S(){if(!(n!=null&&n.base64Png)){a="Select a region first",o();return}await r(async()=>{const i=await s("pin_image",{options:{base64Image:n==null?void 0:n.base64Png}});await s("copy_pin",{id:i.id}),await s("close_pin",{id:i.id}),a="Copied selected image via temporary pin",await p()})}function c(i,t){return typeof i=="number"&&Number.isFinite(i)?i:t}function w(i,t={width:0,height:0}){return{width:c(i==null?void 0:i.width,t.width),height:c(i==null?void 0:i.height,t.height)}}function O(i){return{x:c(i==null?void 0:i.x,0),y:c(i==null?void 0:i.y,0),width:c(i==null?void 0:i.width,0),height:c(i==null?void 0:i.height,0)}}function I(i){if(!i||typeof i.id!="number")return null;const t=w(i.size),d=w(i.displaySize??i.display_size??i.size,t);return{id:i.id,size:t,displaySize:d,position:O(i.position),opacity:c(i.opacity,1)}}async function p(){g=(await s("list_pins")).map(I).filter(t=>t!==null)}async function u(i,t){await r(async()=>{await s("set_pin_opacity",{options:{id:i,opacity:t}}),a=`Set pin ${i} opacity to ${Math.round(t*100)}%`,await p()})}async function P(i){await r(async()=>{await s("close_pin",{id:i}),a=`Closed pin ${i}`,await p()})}async function D(){await r(async()=>{await p(),a=`Loaded ${g.length} pin${g.length===1?"":"s"}`})}function C(){return n!=null&&n.base64Png?`
    <div class="preview-toolbar">
      <button data-action="pin-selection">Pin</button>
      <button data-action="copy-selection">Copy</button>
      <button data-action="clear-selection">Clear</button>
    </div>
    <img class="preview-image" src="data:image/png;base64,${n.base64Png}" alt="Selected capture" />
    <div class="meta">
      ${n.width}x${n.height}
      · screen ${n.rect.x}, ${n.rect.y}
    </div>
  `:'<div class="empty">No selection</div>'}function A(){return g.length===0?'<div class="empty compact">No active pins</div>':g.map(i=>`
        <div class="pin-row">
          <div>
            <strong>#${i.id}</strong>
            <span>${i.displaySize.width}x${i.displaySize.height}</span>
            <span>${i.position.x}, ${i.position.y}</span>
          </div>
          <div class="pin-actions">
            <button data-action="opacity-100" data-id="${i.id}">100%</button>
            <button data-action="opacity-70" data-id="${i.id}">70%</button>
            <button data-action="close-pin" data-id="${i.id}">Close</button>
          </div>
        </div>
      `).join("")}function o(){m.innerHTML=`
    <main>
      <header>
        <div>
          <h1>win-screen Tauri Demo</h1>
          <p>Native overlay selection with a floating WebView toolbar.</p>
        </div>
        <button data-action="select-region" ${y?"disabled":""}>Select Region</button>
      </header>

      <section class="grid">
        <article class="panel preview">
          <h2>Selection</h2>
          ${C()}
        </article>

        <article class="panel">
          <div class="panel-heading">
            <h2>Pins</h2>
            <button data-action="refresh-pins" ${y?"disabled":""}>Refresh</button>
          </div>
          ${A()}
        </article>
      </section>

      <footer>${a}</footer>
    </main>
  `}m.addEventListener("click",i=>{const d=i.target.closest("button[data-action]");if(!d||y)return;const e=d.dataset.action,l=Number(d.dataset.id);e==="select-region"&&R(),e==="pin-selection"&&E(),e==="copy-selection"&&S(),e==="clear-selection"&&(n=null,a="Selection cleared",o()),e==="refresh-pins"&&D(),e==="opacity-100"&&u(l,1),e==="opacity-70"&&u(l,.7),e==="close-pin"&&P(l)});D();b("win-screen-demo://selection-done",async i=>{n=i.payload;const t=i.payload.pinned&&i.payload.pinId?`, pinned #${i.payload.pinId}`:"";a=`Selected ${i.payload.width}x${i.payload.height}${t}`,await p(),o()});b("win-screen-demo://selection-canceled",()=>{a="Selection canceled",o()});o();
