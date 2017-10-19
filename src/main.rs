extern crate daemonize;
extern crate clap;
extern crate time;
extern crate tiny_http;


use daemonize::Daemonize;
use clap::{App, Arg};
use std::env;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process;
use std::str;
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Request, Response, Server};


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

    let dir = env::current_dir().expect("Couldn't read curent directory!");
    let path = {
        if matches.is_present("PATH") {
            Path::new(matches.value_of("PATH").unwrap())
        } else {
            Path::new(dir.to_str().unwrap())
        }
    };

    match run(path, port, daemon, threads) {
        Ok(_) => process::exit(0),
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        },
    }
}


fn run(path: &Path, port: u32, daemonize: bool, threads: usize)
    -> Result<(), String>
{
    if daemonize {
        println!("Forking to background...");
        let status = Daemonize::new().start();
        if status.is_err() {
            return Err(format!("Daemonizing failed! {:?}", status));
        }
    }

    match env::set_current_dir(path) {
        Ok(_) => println!("Serving directory '{}'", path.display()),
        Err(_) => {
            return Err(
                format!("Could not change root to '{}'!", path.display())
                );
        }
    }

    let conf = format!("0.0.0.0:{}", port);
    let server = match Server::http(&conf) {
        Ok(s) => {
            println!("Listening on http:/{}/", conf);
            s
        },
        Err(e) => return Err(
            format!("`Server::http(...)` failed with {:?}!", e)
            ),
    };

    if threads < 2 {
        for request in server.incoming_requests() {
            match handle_request(request) {
                Ok(_) => {},
                Err(e) => {
                    return Err(
                        format!("`request.respond(...)` failed with {:?}!", e)
                        );
                },
            };
        }
    } else {
        let server = Arc::new(server);
        let mut guards = Vec::with_capacity(threads);

        for _ in 0..threads {
            let server = server.clone();

            let guard = thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle_request(request).unwrap();
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
    let url = str::replace(rq.url(), "%20", " ");
    print!("{} '{}'", rq.method().as_str().to_uppercase(), &url);

    match fs::metadata(&(".".to_owned() + &url)) {
        Ok(meta) => {
            if meta.is_dir() {
                respond_dir(rq)
            } else if meta.is_file() {
                respond_file(rq)
            } else {
                respond_404(rq)
            }
        },
        Err(_) => {
            respond_404(rq)
        },
    }
}


fn respond_file(rq: Request) -> Result<(), io::Error> {
    let url = str::replace(rq.url(), "%20", " ");
    let file = File::open(&(".".to_owned() + &url));

    match file {
        Ok(f) => {
            println!(" => 200");
            rq.respond(Response::from_file(f))
        },
        Err(_) => respond_404(rq),
    }
}


fn respond_dir(rq: Request) -> Result<(), io::Error> {
    let url = str::replace(rq.url(), "%20", " ");
    let dir = fs::read_dir(&(".".to_owned() + &url));
    let name = rq.url().to_owned();

    match dir {
        Ok(directory) => {
            println!(" => 200");
            let listing = generate_listing(name, directory);
            let header = Header::from_bytes(
                "Content-Type".as_bytes(),
                "text/html".as_bytes())
                .unwrap();

            rq.respond(Response::from_string(listing).with_header(header))
        },
        Err(_) => {
            // println!("{:?}", e);
            respond_404(rq)
        },
    }
}


fn respond_404(rq: Request) -> Result<(), io::Error> {
    println!(" => 404");
    let url = rq.url().to_owned();
    let content = format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <title>404 Not Found</title>\n\
         </head>\n\
         <body>\n\
         <h1>Not found - {}</h1>\n\
         </body>\n\
         </html>",
         url);


    let header = Header::from_bytes(
        "Content-Type".as_bytes(),
        "text/html".as_bytes())
        .unwrap();

    let response = Response::from_string(content)
        .with_header(header)
        .with_status_code(404);

    rq.respond(response)
}


fn generate_listing(name: String, dir: fs::ReadDir) -> String {
    let path = Path::new(&name);
    let dotdot = path.parent().unwrap_or(Path::new(&".."));
    let mut listing = String::from(
        format!("<!DOCTYPE html>\n\
        <html>\n\
        <head>\n\
        <title>{}</title>\n\
        </head>\n\
        <body>\n\
        <h1>{}</h1>\n\
        <tt><pre>\n\
        <table>\n\
        <tr><td><a href=\"{}\">..</a></td></tr>\n\
        ",
        name,
        name,
        dotdot.display()
        ));

    for entry in dir {
        let entry = entry.expect("Failed to read entry!");
        let name = entry.file_name().into_string().unwrap();
        let meta = entry.metadata().expect("Failed to read metadata!");

        let path = entry.path();
        let path = path.strip_prefix(".").expect("Failed to strip prefix!");
        
        if meta.is_dir() {
            let string = format!(
                "<tr><td><a href=\"/{}\">{}/</a></td><td></td></tr>\n",
                path.display(),
                name
                );
            listing.push_str(&string);
        } else {
            let size = meta.len();
            let string = format!(
                "<tr><td><a href=\"/{}\">{}</a></td>\
                <td align=\"right\">   {}</td></tr>\n",
                path.display(),
                name,
                size
                );
            listing.push_str(&string);
        }
    }

    listing.push_str("</table>\n</pre></tt>\n");

    let now = time::now();
    let now = now.to_utc();
    let now = now.asctime();

    listing.push_str(
        &format!("<hr>\nGenerated on {} UTC\n</body>\n</html>", now)
        );

    listing
}
