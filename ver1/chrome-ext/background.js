const ws_url = "ws://127.0.0.1:24810"

let ws = null;
let tabsGlobal = null
async function conn() {
    try {
        ws = new WebSocket(ws_url);
        console.log(typeof WebSocket)
        ws.onopen = ()=> console.log("opening");
        ws.onclose = ()=> {
            console.log("close")
            setTimeout(conn , 2000)
        };
        ws.onmessage = (event) =>{
            const data = JSON.parse(event.data);
            if (data.action === "get_tabs") {
                console.log("send back tabs")
                ws.send(JSON.stringify({action : "tabs" , tabs : tabsGlobal || []}))
                
            } else if (data.action === "tel") {
                const windowId = null
                chrome.windows.create({
                    focused: true
                } , (win)=> windowId = win.id);

                for (const url of data.urls) {
                    chrome.tabs.create({
                        windowId: windowId,
                        url: url,
                        active: true
                    });
                }
            }
        }
    } catch (err) {
        console.error("conn : " + err)
    }
}

conn()


async function update() {
    try {
        const [tab] = await chrome.tabs.query({active : true , currentWindow : true});
        if (tab) {
            let tabs = await chrome.tabs.query({ windowId: tab.windowId })
            if (tabs) {
                tabsGlobal = tabs.map(val => val.url);
            }
        }
    } catch (err) {
        console.error("update" , err);
    }
}



chrome.tabs.onActivated.addListener(update);
chrome.tabs.onUpdated.addListener(update);
chrome.windows.onFocusChanged.addListener(update);