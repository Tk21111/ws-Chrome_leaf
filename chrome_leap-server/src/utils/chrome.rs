use std::process::Command;

pub fn open_chrome(urls : Vec<String>) {

    println!("URLs to open: {:?}", urls);

    for url in urls {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "chrome", url])
                .spawn()
                .expect("failed to open chrome");
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("google-chrome")
                .args([url])
                .spawn()
                .expect("failed to open chrome");
        }
    }

}