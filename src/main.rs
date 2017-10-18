extern crate daemonize;
extern crate clap;
extern crate tiny_http;


use daemonize::Daemonize;
use clap::{App, Arg};
use std::env;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process;
use std::sync::Arc;
use std::thread;
use tiny_http::{Server, Request, Response};


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
             .help("Detach from terminal and run in background."))
        .arg(Arg::with_name("threads")
             .short("t")
             .long("threads")
             .takes_value(true)
             .help("Number of threads to work with. Default 1."));

    let matches = app.clone().get_matches();

    if matches.is_present("PATH") {
        let path = Path::new(matches.value_of("PATH").unwrap());
        match env::set_current_dir(&path) {
            Ok(_) => println!("Serving directory '{}'...", path.display()),
            Err(_) => {
                println!("Could not change root to '{}'!", path.display());
                process::exit(1);
            },
        };
    }

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

    let threads = matches.value_of("threads").unwrap_or("1");
    let threads = match threads.parse::<usize>() {
        Ok(num) => num,
        Err(_) => {
            println!("Threads must be a number!\n");
            app.print_help().unwrap();
            println!();
            process::exit(1);
        },
    };

    match run(port, daemon, threads) {
        Ok(_) => process::exit(0),
        Err(e) => {
            println!("{:?}", e);
            process::exit(1);
        },
    }
}


fn run(port: u32, daemonize: bool, threads: usize) -> Result<(), String> {
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

    if threads < 2 {
        for request in server.incoming_requests() {
            match handle_request(request) {
                Ok(_) => {},
                Err(e) => return Err(format!("`request.respond(...)` failed with {:?}!", e)),
            };
        }
    } else {
        let server = Arc::new(server);
        let mut guards = Vec::with_capacity(threads);

        for _ in 0..threads {
            let server = server.clone();

            let guard = thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle_request(rq).unwrap();
                }
            });

            guards.push(guard);
        }

        for guard in guards {
            guard.join().unwrap();
        }
    }

    Ok(())
}


fn handle_request(rq: Request) -> Result<(), io::Error> {
    let file = File::open(&Path::new(&(".".to_owned() + rq.url())));

    print!("{} {}", rq.method().as_str().to_uppercase(), rq.url());

    match file {
        Ok(f) => {
            println!(" => 200");
            rq.respond(Response::from_file(f))
        },
        Err(_) => {
            println!(" => 404");
            rq.respond(Response::empty(404))
        },
    }
}
