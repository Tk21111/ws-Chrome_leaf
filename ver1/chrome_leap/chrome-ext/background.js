const ws_url = "ws://127.0.0.1:24810"

let ws = null;
let tabsGlobal = null
let reconnTimeout = 1000;
async function conn() {
    try {
        ws = new WebSocket(ws_url);

        ws.onopen = ()=> {
            console.log(" [ws] - connected");
        };

        ws.onclose = ()=> {
            console.log(" [ws] - connection close");
            reconnTimeout = Math.min(reconnTimeout * 2 , 10000);
            setTimeout(conn , reconnTimeout);
        };

        ws.onmessage = (event) =>{

            try {
                const data = JSON.parse(event.data);
                if (data.action === "get_tabs") {

                    console.log(tabsGlobal);
                    console.log(data)
                    ws.send(JSON.stringify({action : "tabs" , tabs : tabsGlobal || [] , edge : data.edge || ""}));
                    
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
            } catch (err) {
                console.error("[msg ws] - error : " , err);
            }   
        };

        ws.onerror = (err) => {
            console.warn("[ws] - error : " , err);
            ws.close();
        };
    } catch (err) {
        console.error("[ws] - connection error : " + err)
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