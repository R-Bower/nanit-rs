use std::io::{self, BufRead, Write};

pub fn prompt_input(prompt: &str) -> io::Result<String> {
    eprint!("{}", prompt);
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end().to_string())
}

pub fn get_local_ip() -> Option<String> {
    local_ip_address::local_ip()
        .ok()
        .map(|ip| ip.to_string())
}
