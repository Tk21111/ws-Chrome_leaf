## PROJECT
> simple tool for detect chrome at edge and send to other device in sam LAN

## REQUIREMENT
(linux) => {
    X11
    note linux -> use Xorg -> gear on log in page
}

## TODO
- [x] Add window support
- [x] Add Linux support 
- [x] Reconnect , Retry
- [ ] Add multiple device support
- [x] multi screen support

## VERSION

ver 1 - py
ver 2 - rust 

## bug 

Error: trailing characters at line 1 column 207
error: process didn't exit successfully: `target\debug\tcp-test.exe` (exit code: 1)
use serde_json::{Deserializer, Value};

potential fix
fn main() {
    // Your raw concatenated string
    let data = r#"["http://0.0.0.0:8000/","chrome://extensions/"]["http://0.0.0.0:8000/","chrome://extensions/"]"#;

    // Create a stream iterator
    let stream = Deserializer::from_str(data).into_iter::<Vec<String>>();

    // Iterate through each JSON array found in the string
    for result in stream {
        match result {
            Ok(url_list) => {
                println!("Parsed a list: {:?}", url_list);
                // Do something with the Vec<String> here
            }
            Err(e) => eprintln!("Parsing error: {}", e),
        }
    }
}
in client 

??? add buffer between send is most likely a fix not this
maybe compare to prev and not send