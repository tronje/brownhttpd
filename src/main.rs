extern crate daemonize;
extern crate clap;
extern crate libc;
extern crate time;
extern crate tiny_http;


use daemonize::Daemonize;


use std::env;
use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::net::{Ipv6Addr, SocketAddrV6};
use std::path::{Path, PathBuf};
use std::process;
use std::str;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Request, Response, Server, StatusCode};


mod cli;


fn main() {
    let mut app = cli::build_cli();

    let matches = app.clone().get_matches();

    // when generating completions, do that and nothing else!
    if matches.is_present("gen_completions") {
        use clap::Shell::{Bash, Zsh, Fish};
        let shell = matches.value_of("gen_completions").unwrap();
        let name = "brownhttpd";
        match shell {
            "bash" => app.gen_completions_to(name, Bash, &mut io::stdout()),
            "zsh" => app.gen_completions_to(name, Zsh, &mut io::stdout()),
            "fish" => app.gen_completions_to(name, Fish, &mut io::stdout()),
            _ => {
                println!("Unknown shell '{}'!", shell);
                process::exit(1);
            },
        }

        return;
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

    let ipv6 = matches.is_present("ipv6");

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

    let path = {
        if matches.is_present("PATH") {
            PathBuf::from(matches.value_of("PATH").unwrap())
        } else {
            env::current_dir().expect("Couldn't read curent directory!")
        }
    };

    let index = matches.value_of("index").unwrap_or("index.html");
    let index = index.to_owned();

    let chroot = matches.is_present("chroot");

    match run(
        path.as_path(),
        port,
        ipv6,
        chroot,
        daemon,
        threads,
        index
    ) {
        Ok(_) => process::exit(0),
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        },
    }
}


fn run(
    path: &Path,
    port: u32,
    ipv6: bool,
    chroot: bool,
    daemonize: bool,
    threads: usize,
    index: String
) -> Result<(), String> {
    if daemonize {
        println!("Forking to background...");
        let status = Daemonize::new().start();
        if status.is_err() {
            return Err(format!("Daemonizing failed! {:?}", status));
        }
    }

    match env::set_current_dir(path) {
        Ok(_) => {}, 
        Err(_) => {
            return Err(
                format!("Could not change root to '{}'!", path.display())
                );
        }
    }

    if chroot {
        let c_path = CString::new(path.to_str().unwrap())
            .expect("Constructing chroot path failed!");
        let c_path_ptr = c_path.as_ptr();

        let chroot_status = unsafe {
            libc::chroot(c_path_ptr)
        };

        if chroot_status != 0 {
            return Err(format!("Chrooting failed with code {}!", chroot_status));
        } else {
            println!("Chrooted to '{}'", path.display());
        }
    }

    println!("Serving directory '{}'", path.display());

    // create server
    // either listen on IPv6 localhost, or IPv4 localhost
    let server = if ipv6 {
        let socket = SocketAddrV6::new(
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
            port as u16,
            0,
            0
        );

        match Server::http(&socket) {
            Ok(s) => {
                println!("Listening on http:/{:?}/", socket);
                s
            },
            Err(e) => return Err(
                format!("`Server::http(...)` failed with {:?}", e)
                ),
        }
    } else {
        let conf = format!("0.0.0.0:{}", port);
        
        match Server::http(&conf) {
            Ok(s) => {
                println!("Listening on http:/{}/", conf);
                s
            },
            Err(e) => return Err(
                format!("`Server::http(...)` failed with {:?}!", e)
                ),
        }
    };

    if threads < 2 {
        for request in server.incoming_requests() {
            match handle_request(request, index.as_str()) {
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
        let index = Arc::new(index);
        let mut guards = Vec::with_capacity(threads);

        for _ in 0..threads {
            let server = server.clone();
            let index = index.clone();

            let guard = thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle_request(request, index.as_str()).unwrap();
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


fn handle_request(rq: Request, index: &str) -> Result<(), io::Error> {
    let url = str::replace(rq.url(), "%20", " ");
    print!("{} '{}'", rq.method().as_str().to_uppercase(), &url);

    match fs::metadata(&(".".to_owned() + &url)) {
        Ok(meta) => {
            if meta.is_dir() {
                respond_dir(rq, index)
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


fn respond_dir(rq: Request, index: &str) -> Result<(), io::Error> {
    let url = str::replace(rq.url(), "%20", " ");
    let dir = fs::read_dir(&(".".to_owned() + &url));
    let name = rq.url().to_owned();

    match dir {
        Ok(directory) => {
            match File::open(format!(".{}{}", url, index)) {
                Ok(f) => {
                    // respond with index file
                    println!(" => 200");
                    rq.respond(Response::from_file(f))
                },
                Err(_) => {
                    // respond with directory listing
                    let listing = generate_listing(name, directory);
                    let header = Header::from_bytes(
                        "Content-Type".as_bytes(),
                        "text/html".as_bytes())
                        .unwrap();
                    let response = Response::new(
                        StatusCode(200),
                        vec![header],
                        listing.as_bytes(),
                        Some(listing.len()),
                        None);

                    println!(" => 200");
                    rq.respond(response)
                }
            }
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
