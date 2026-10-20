use std::{thread, time::Duration};

use soupbintcp_client::{LoginRequest, SoupBinTCPClient};

fn main() {
    println!("starting client");

    let client = SoupBinTCPClient::connect("127.0.0.1:21300").expect("failed to connect");

    let (session, rx) = client
        .login(LoginRequest::new("USER", "P@SSWORD1", "", "1"))
        .expect("failed to login");

    println!(
        "logged in: session={} seq={}",
        session.session_id, session.sequence_number
    );

    let printer = thread::spawn(move || {
        for event in rx {
            println!("Received: {:?}", event);
        }
    });

    thread::sleep(Duration::from_secs(1));
    session.logout().expect("failed to logout");

    printer.join().expect("printer thread panicked");
}
