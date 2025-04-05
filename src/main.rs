use std::env;
use std::path::Path;
use std::process::exit;
use std::fs::File;
use std::io::Read; // Import the Read trait
use std::io::Error; // For error handling
use fancy_regex::Regex;
//use reqwest::Error;
//use md5;

// Read command line args:
// - First arg is the directory containing the target OS
fn main() -> Result<(), Error> {
    // Get args
    let args: Vec<String> = env::args().collect();
    dbg!(&args);

    let mut target = args[1].to_owned();

    //Verify the provided directory exists
    //println!("{}", Path::new(target).exists());
    if Path::new(&target).exists(){
        println!("[*] Testing for {}", target);
    }
    else{
        println!("[x] Folder was not found, exiting...");
        exit(0)
    }

    // Find /etc/os-release
    target.push_str("/etc/os-release");
    if Path::new(&target).exists(){
        println!("[*] Found {}", target);
    }
    else{
        println!("[x] /etc/os-release was not found in the target file system, if it is stored elsewhere move it to this location or create it if it is missing, exiting...");
        exit(0)
    }

    // Open /etc/os-release
    let mut file = File::open(target)?;
    let mut contents = String::new();
    let mut ids: Vec<&str> = Vec::new(); 
    file.read_to_string(&mut contents)?;
        
    // Parse VERSION_ID and ID
    let re = Regex::new(r"(?<=ID=).*").unwrap();
    for match_result in re.find_iter(&contents) {
        match match_result {
            Ok(some_match) => {
                println!("[*] {}", 
                    &contents[some_match.start()..some_match.end()]
                );
                ids.push(&contents[some_match.start()..some_match.end()]);
            },
            Err(e) => println!("Error during matching: {}", e),
        }
    }

    let distro = ids[0];
    let version = ids[1];

    // Match the url for downloading the matching release of the target
    // Download from the target url
    // For debian
    if distro.contains("debian") {
        println!("[*] Target is debian");
    }
    // For Ubuntu
    else if distro.contains("ubuntu") {
        println!("[*] Target is ubuntu");
    }
    else if distro.contains("alpine") {
        println!("[*] Target is alpine");
        // https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/alpine-minirootfs-VERSION-x86_64.tar.gz

    }
    // For other linux (alpine)
    else {

        println!("[*] Not Debian or Ubuntu, defaulting to latest alpine...");
        // https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/alpine-minirootfs-3.21.0-x86_64.tar.gz
    }

    // Decompress the downloaded release

    // For each file/directory in the user-supplied OS directory verify if it is present in the download, if file is not present then save file name to results

    // For each files that exist in both, compare hashes, if hashes do not match then save the file name to results
    Ok(())
}
