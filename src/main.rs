use std::env;
use std::io::Write;
use std::path::Path;
use std::process::exit;
use std::fs::File;
use std::io::Read; // Import the Read trait
use fancy_regex::Regex;
use reqwest::*;
use flate2::read::GzDecoder;
use tar::Archive;
use std::fs;
use tokio::runtime::Runtime;
//use std::fs::DirEntry;

// Read command line args:
// - First arg is the directory containing the target OS
fn main() -> Result<()> {

    // Create the runtime
    let rt  = Runtime::new().unwrap();

    // Get args
    let args: Vec<String> = env::args().collect();
    dbg!(&args);

    let mut target = args[1].to_owned();

    println!("[!] This program is ONLY intended for x86/64 images currently");

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
    let mut file = File::open(target).unwrap();
    let mut contents = String::new();
    let mut ids: Vec<&str> = Vec::new(); 
    file.read_to_string(&mut contents).unwrap();
        
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
    let client = Client::new();
    let mut path = String::new();
    let mut file:File;
    let mut response = rt.block_on(client.get("https://cloud-images.ubuntu.com/releases/{version_name}/release/{path}")
        .send()).unwrap();
    // For debian
    if distro.contains("debian") {
        println!("[*] Target is debian");
        // NEED TO GET VERSION NAME
        //https://ftp.freedombox.org/pub/freedombox/archive/stable-2019.07/hardware/virtualbox-amd64/stable/
    }
    // For Ubuntu
    else if distro.contains("ubuntu") {
        println!("[*] Target is ubuntu");
        path = format!("ubuntu-{}-server-cloudimg-amd64.tar.gz",version);
        // NEED TO GET VERSION NAME
    }
    // Other or alpine
    else {
        println!("[*] Target is alpine or other, defaulting to alpine");
        path = format!("alpine-minirootfs-{}-x86_64.tar.gz",version);
        let url = format!("https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/{}",path);
        response = rt.block_on(get(url)).unwrap();
    }

    file = File::create(&path).unwrap();

    if !response.status().is_success() {
        panic!("failed to get a successful response status!");
    }

    let body = rt.block_on(response.bytes()).unwrap();
    file.write_all(&body).unwrap();

    println!("[*] {} downloaded successfully!",&path);

    fs::create_dir("./temp").unwrap();

    println!("[*] Decompressing {}..",&path);
    let tar_gz = File::open(&path).unwrap();
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack("./temp").unwrap();

    // For each file/directory in the user-supplied OS directory verify if it is present in the download, if file is not present then save file name to results


    // For each files that exist in both, compare hashes, if hashes do not match then save the file name to results

    println!("[*] Deleting artifacts {path}..");
    fs::remove_dir_all("./temp").unwrap();
    fs::remove_file(path).unwrap();

    Ok(())
}
