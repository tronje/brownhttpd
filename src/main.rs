extern crate daemonize;
extern crate clap;
extern crate tiny_http;


use daemonize::Daemonize;
use clap::{App, Arg};
use std::process;
use tiny_http::{Server, Response};


fn run(port: u32, daemonize: bool) -> Result<(), String> {
    if daemonize {
        println!("Forking to background...");
        let status = Daemonize::new().start();
        if status.is_err() {
            return Err(format!("Daemonizing failed! {:?}", status));
        }
    }

    let _conf = format!("0.0.0.0:{}", port);
    let server = match Server::http(_conf) {
        Ok(s) => s,
        Err(e) => return Err(format!("`Server::http(...)` failed with {:?}!", e)),
    };

    for request in server.incoming_requests() {
        println!("received request! method: {:?}, url: {:?}, headers: {:?}",
                 request.method(),
                 request.url(),
                 request.headers()
                );

        let response = Response::from_string("hello world");
        match request.respond(response) {
            Ok(_) => continue,
            Err(e) => return Err(format!("`request.respond(...)` failed with {:?}!", e)),
        };
    }

    Ok(())
}


fn main() {
    let mut app = App::new("brownhttpd")
        .version("0.1.0")
        .author("Tronje Krabbe <hi@tron.je>")
        .about("Simple http server")
        .arg(Arg::with_name("PATH")
             .help("Directory to serve, defaults to current directory.")
             .required(false)
             .index(1))
        .arg(Arg::with_name("port")
             .short("p")
             .long("port")
             .value_name("PORT")
             .help("Set the port to listen on, defaults to 7878.")
             .takes_value(true))
        .arg(Arg::with_name("daemon")
             .short("d")
             .long("daemon")
             .takes_value(false)
             .help("Detach from terminal and run in background."));

    let matches = app.clone().get_matches();

    let port = matches.value_of("port").unwrap_or("7878");
    let port = match port.parse::<u32>() {
        Ok(num) => num,
        Err(_) => {
            println!("Port must be a number!\n");
            app.print_help().unwrap();
            // without this, the help won't be printed entirely
            // not sure why...
            println!();
            process::exit(1);
        },
    };

    let daemon = matches.is_present("daemon");

    match run(port, daemon) {
        Ok(_) => process::exit(0),
        Err(e) => {
            println!("{:?}", e);
            process::exit(1);
        },
    }
}
