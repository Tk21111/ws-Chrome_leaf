use rdev::{listen, Event, EventType, Button, display_size};
use std::{sync::{
    Arc, 
    atomic::{AtomicBool, Ordering}
}, time::Duration};
use std::time::{Instant};

//Send save to send to another thread
//Sync save to share between thread
//static lifetime must remain constant ( life ) during program

pub enum Edge {
    Left,
    Right,
}

pub fn edge_check<F>(on_edge : F) where F : Fn(Edge) + Send + Sync + 'static{

    let draging = Arc::new(AtomicBool::new(false)); 
    let drag_start = Arc::new(std::sync::Mutex::new(Instant::now()));
    let last_trigger = Arc::new(std::sync::Mutex::new(Instant::now() - Duration::from_secs(10))); // start in the past


    let draging_thread = draging.clone();
    let drag_start_thread = drag_start.clone();

    let on_edge = Arc::new(on_edge);
    let on_edge_clone = on_edge.clone();
    let cooldown = 10000;

    std::thread::spawn(move || {

        listen(move | event :Event |{
            // println!("{:?}", event);
            match event.event_type {
                EventType::ButtonPress(Button::Left) => {
                    draging_thread.store(true , Ordering::SeqCst);
                    //get pass ref and reset the data not change it 
                    *drag_start_thread.lock().unwrap() = Instant::now();
                }

                EventType::ButtonRelease(Button::Left) => {
                    draging_thread.store(false, Ordering::SeqCst);
                }

                EventType::MouseMove { x, y } => {
                    if draging_thread.load(Ordering::SeqCst) {
                        let ( screen_w_u64 , screen_h_u64) = display_size().unwrap();
                        let screen_w = screen_w_u64 as f64;
                        let screen_h = screen_h_u64 as f64;
                        let edge_screen = 15.0;

                        let x_at_left = x <= edge_screen;
                        let x_at_right = x >= screen_w - edge_screen;

                        // println!("{:?}" , x);
                        // println!("{:?}" , screen_w);
                        if x_at_left || x_at_right {
                            let held_for  =drag_start_thread.lock().unwrap().elapsed().as_millis();
                            let mut last = last_trigger.lock().unwrap();
                            // println!("{:?}" ,is_active_window_chrome());
                            if held_for > 300 && is_active_window_chrome() && last.elapsed().as_millis() > cooldown{
                                if x_at_left {
                                    // println!("left");
                                    on_edge_clone(Edge::Left);
                                } else {
                                    // println!("right");
                                    on_edge_clone(Edge::Right);
                                }
                                *last = Instant::now();
                            }
                        }
                    }
                }
                _ => {}
            }
        }).expect("mouse hook failed");
    });
}


#[cfg(target_os = "windows")]
fn is_active_window_chrome() -> bool {
    use windows::Win32::{
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        System::ProcessStatus::K32GetModuleBaseNameA,
        System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        Foundation::{CloseHandle, MAX_PATH, HWND},
    };

    unsafe {
        let fore_ground_window = GetForegroundWindow();
        
        // HWND wraps an isize, so checking .0 == 0 works, but HWND(0) is cleaner
        if fore_ground_window == HWND(std::ptr::null_mut()) {
            return false;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(fore_ground_window, Some(&mut pid));

        if pid == 0 {
            return false;
        }

        // FIX: OpenProcess returns Result<HANDLE>. Handle the Result, don't check for 0.
        let process_handle = match OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 
            false, 
            pid
        ) {
            Ok(handle) => handle, // If successful, we get the HANDLE
            Err(_) => return false, // If it fails (access denied, etc.), return false
        };

        let mut name_buf = [0u8; MAX_PATH as usize];
        
        let len = K32GetModuleBaseNameA(
            process_handle,
            None, // Use None instead of 0 for null HMODULE
            &mut name_buf,
        );

        // Important: Close the handle to avoid memory leaks!
        let _ = CloseHandle(process_handle);

        if len == 0 {
            return false;
        }

        let name = String::from_utf8_lossy(&name_buf[..len as usize]).to_lowercase();   
        name.contains("chrome.exe") || name.contains("chrome")
    }
}

#[cfg(target_os = "linux")]
fn is_active_window_chrome() -> bool { 
   
   use x_win::get_active_window;

   match get_active_window() {
       Ok(window) => {
            //collection turn iteration -> vector 
            let parts : Vec<&str> = window.title.split("-").collect();

            if let Some(last) = parts.last() {
                let app_name = last.trim();
                return app_name == "Google Chrome";
            }

            false
       }

       Err(_) => {
            eprintln!("Error getting active window");
            false
        }
   }
}

