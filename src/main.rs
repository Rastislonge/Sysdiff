use std::env;
use std::io::Write;
use std::path::Path;
use std::process::exit;
use std::fs::File;
use std::io::Read;
use fancy_regex::Regex;
use reqwest::Client;
use flate2::read::GzDecoder;
use tar::Archive;
use std::fs;
use tokio::runtime::Runtime;
extern crate walkdir;
use walkdir::WalkDir;
use std::collections::HashMap;
use log::LevelFilter;
use log::{debug, info};
use guestfs::{Handle, AddDriveOptArgs, IsDirOptArgs, UmountOptArgs};
use xz2::read::XzDecoder;

use env_logger::Builder;

// Get version codename
fn get_codename(contents: &str) -> Option<String> {
    let re = Regex::new(r"^VERSION_CODENAME=(.*)$").unwrap();
    for line in contents.lines() {
        if let Ok(Some(captures)) = re.captures(line) {
            if let Some(codename_match) = captures.get(1) {
                let codename = codename_match.as_str().trim_matches('"').to_string();
                info!("[*] Codename: {}", codename);
                return Some(codename);
            }
        }
    }
    None
}

// Extract all files from fs
fn extract_all_files(g: &Handle, source_path: &str, dest_dir: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let mut file_count = 0;
    
    // Get list of files in current directory
    let entries = match g.ls(source_path) {
        Ok(entries) => entries,
        Err(e) => {
            println!("    Warning: Cannot list {}: {:?}", source_path, e);
            return Ok(0);
        }
    };
    
    for entry in entries {
        // Skip . and .. entries
        if entry == "." || entry == ".." {
            continue;
        }
        
        let full_source_path = if source_path == "/" {
            format!("/{}", entry)
        } else {
            format!("{}/{}", source_path, entry)
        };
        
        let full_dest_path = format!("{}/{}", dest_dir, entry);
        
        // Check if it's a directory or file
        match g.is_dir(&full_source_path, IsDirOptArgs::default()) {
            Ok(true) => {
                // It's a directory - create it and recurse
                println!("    Creating directory: {}", full_dest_path);
                fs::create_dir_all(&full_dest_path)?;
                
                // Recursively extract contents
                let sub_count = extract_all_files(g, &full_source_path, &full_dest_path)?;
                file_count += sub_count;
            }
            Ok(false) => {
                // It's a file - copy it out
                match g.download(&full_source_path, &full_dest_path) {
                    Ok(_) => {
                        println!("    Extracted file: {}", full_dest_path);
                        file_count += 1;
                    }
                    Err(e) => {
                        println!("    Warning: Failed to extract {}: {:?}", full_source_path, e);
                    }
                }
            }
            Err(e) => {
                println!("    Warning: Cannot determine type of {}: {:?}", full_source_path, e);
            }
        }
    }
    
    Ok(file_count)
}

// Extract fs from qcow2
fn extract_qcow2(qcow2_path: &str, second_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let g = Handle::create().map_err(|e| format!("Failed to create guestfs handle: {:?}", e))?;
    
    g.add_drive(qcow2_path, AddDriveOptArgs::default())
        .map_err(|e| format!("Failed to add drive: {:?}", e))?;
    
    g.launch().map_err(|e| format!("Failed to launch guestfs: {:?}", e))?;

    // List filesystems
    let filesystems = g.list_filesystems()
        .map_err(|e| format!("Failed to list filesystems: {:?}", e))?;
        
    for (device, fstype) in filesystems {
        println!("Device: {}, Type: {}", device, fstype);
        
        if fstype != "unknown" {
            g.mount(&device, "/")
                .map_err(|e| format!("Failed to mount {}: {:?}", device, e))?;
            
            // Extract all files recursively
            match extract_all_files(&g, "/", second_dir) {
                Ok(file_count) => {
                    println!("  Extracted {} items from {}", file_count, device);
                }
                Err(e) => {
                    println!("  Error extracting from {}: {}", device, e);
                }
            }
                
            // Unmount
            match g.umount("/", UmountOptArgs::default()) {
                Ok(_) => println!("  Unmounted {}", device),
                Err(e) => println!("  Warning: Failed to unmount {}: {:?}", device, e),
            }
        }
    }
    
    Ok(())
}

// Compare hashmaps
fn find_unique_keys(dl_target_file_hashes: &HashMap<md5::Digest, String>, usr_target_file_hashes: &HashMap<md5::Digest, String>) -> Vec<String> {
    let only_in_usr: Vec<String> = usr_target_file_hashes.iter()
        .filter(|(key, _)| !dl_target_file_hashes.contains_key(*key))
        .map(|(_, value)| value.clone())
        .collect();

    only_in_usr
}

// Read command line args:
// - First arg is the directory containing the target OS
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the runtime
    let rt = Runtime::new().unwrap();

    // Some globals
    let mut downloaded_file_path = String::new();

    // Get args
    let args: Vec<String> = env::args().collect();

    let mut target = args[1].to_owned();

    println!("[!] This program is ONLY intended for x86/64 images currently");
    
    // Validate args and handle options
    let help: &'static str = "Usage: sysdiff <PATH TO TARGET FILESYSTEM> <OPTIONAL ARGS>\n\n example:  sysdiff ./target_fs --file output.txt\n\n --help: print this message\n --verbose: <LEVEL>\n     1 = Print main program operations\n     2 = Print hashes and file paths\n    other = 1\n --file: choose output file for unique file names\n --symlink: handle comparison of files referred to by symlinks\n --user <path_to_second_target_fs>: Compare the main filesystem to a second user-provided file system rather than querying online sources";
    let mut output_file_path = String::new();
    let mut arch = String::new();
    let mut target2 = String::new();
    let mut second_dir = String::from("./temp");
    let mut symlink = false;
    
    if args.len() > 1 {
        if args.iter().any(|arg| arg == "--help") {
            println!("{}", help);
            exit(0)
        }
        
        if let Some(index) = args.iter().position(|arg| arg == "--verbose") {
            let log_level = if index + 1 < args.len() {
            match args[index + 1].as_str() {
                "1" => LevelFilter::Info,
                "2" => LevelFilter::Debug,
                _ => LevelFilter::Info,
            }
        } else {
            LevelFilter::Info  // default log level
          };
    
        Builder::from_default_env()
            .filter_level(log_level)
            .init();
        }
        
        if let Some(index) = args.iter().position(|arg| arg == "--file") {
            if index + 1 < args.len() {
                output_file_path = args[index + 1].to_owned();
            } else {
                panic!("[x] Missing filename!");
            }
        }
        
        if let Some(index) = args.iter().position(|arg| arg == "--arch") {
            if index + 1 < args.len() {
                arch = args[index + 1].to_owned();
            } else {
                panic!("[x] Missing arch!");
            }
        } else {
            arch = ("x86_64").to_string();
        }
        
        if args.iter().any(|arg| arg == "--symlink") {
            symlink = true
        }

        if let Some(index) = args.iter().position(|arg| arg == "--user") {
            if index + 1 < args.len() {
                target2 = args[index + 1].to_owned();
                info!("[*] Using two user-provided directories");
            } else {
                panic!("[x] Missing second directory!");
            }
        }

    } else {
        println!("{}", help);
        exit(0)
    }
    
    //Verify the provided directory exists
    if Path::new(&target).exists() {
        info!("[*] Testing for {}", target);
    }
    else {
        panic!("[x] Folder was not found, exiting...");
    }

    //If needed verify the existence of the second directory
    if !target2.is_empty() {
        if Path::new(&target2).exists(){
            info!("[*] Testing for the second directory of {}", &target2);
            second_dir = target2.clone();
        }
        else {
            panic!("[x] Folder for --user was not found, exiting...");
        }
    } else {
        // Find /etc/os-release
        let mut distro = String::new();
        let mut version = String::new();
        let mut contents = String::new();
        target.push_str("/etc/os-release");

        if Path::new(&target).exists(){
            info!("[*] Found {}", target);
            // Open /etc/os-release
            let mut file = File::open(&target).unwrap();
            file.read_to_string(&mut contents).unwrap();
        
            // Match ID (but not VERSION_ID)
            let id_re = Regex::new(r"^ID=(.*)$").unwrap();
            for line in contents.lines() {
                if let Ok(Some(captures)) = id_re.captures(line) {
                    if let Some(id_match) = captures.get(1) {
                        distro = id_match.as_str().trim_matches('"').to_string();
                        info!("[*] Distribution ID: {}", distro);
                        break;
                    }
                }
            }
        
            // Match VERSION_ID
            let version_re = Regex::new(r"^VERSION_ID=(.*)$").unwrap();
            for line in contents.lines() {
                if let Ok(Some(captures)) = version_re.captures(line) {
                    if let Some(version_match) = captures.get(1) {
                        version = version_match.as_str().trim_matches('"').to_string();
                        info!("[*] Version ID: {}", version);
                        break;
                    }
                }
            }
        }
        else{
            info!("[!] /etc/os-release was not found in the target file system, if it is stored elsewhere move it to this location or create it if it is missing, exiting...");
        }
        
        if distro.is_empty() || version.is_empty() {
            info!("[!] Could not parse ID or VERSION_ID from os-release file");
        }

        // Match the url for downloading the matching release of the target
        // Download from the target url
        let url;
        let mut is_qcow2 = false;
        let mut is_gz = false;
        
        // For debian
        if distro.contains("debian") {
            info!("[*] Target is debian");
            let codename = get_codename(&contents).unwrap_or_else(|| {
                panic!("[x] Could not find VERSION_CODENAME for Debian");
            });
            let mut arch_name = arch.clone();
            if arch == "x86_64" {
                arch_name = "amd64".to_string();
            }
            // Defaults to latest!
            downloaded_file_path = format!("debian-{}-generic-{}.qcow2", version, arch_name);
            url = format!("https://cloud.debian.org/images/cloud/{}/latest/{}", codename, downloaded_file_path);
            is_qcow2 = true;
        }
        // For Ubuntu
        else if distro.contains("ubuntu") {
            info!("[*] Target is ubuntu");
            let codename = get_codename(&contents).unwrap_or_else(|| {
                panic!("[x] Could not find VERSION_CODENAME for Ubuntu");
            });
            let mut arch_name = arch.clone();
            if arch == "x86_64" {
                arch_name = "amd64".to_string();
            }
            downloaded_file_path = format!("ubuntu-{}-server-cloudimg-{}.qcow2", version, arch_name);
            url = format!("https://cloud-images.ubuntu.com/releases/{}/release/{}", codename, downloaded_file_path);
            is_qcow2 = true;
        }
        // Alpine
        else if distro.contains("alpine") {
            info!("[*] Target is alpine");
            downloaded_file_path = format!("alpine-minirootfs-{}-{}.tar.gz", version, arch);
            let version_short = &version[..version.len().saturating_sub(2)];
            url = format!("https://dl-cdn.alpinelinux.org/alpine/v{}/releases/{}/{}", version_short, arch, downloaded_file_path);
            is_gz = true;
        }
        // Other
        else {
            info!("[*] Target is not supported or undefined, defaulting to buildroot fs");
            downloaded_file_path = "buildroot-2025.05.tar.xz".to_string();
            url = "https://buildroot.org/downloads/buildroot-2025.05.tar.xz".to_string();
        }

        info!("[*] Downloading from: {}", url);
        let response = rt.block_on(reqwest::get(&url))?;

        let mut file = File::create(&downloaded_file_path)?;

        if !response.status().is_success() {
            panic!("[x] failed to get a successful response status! If you've specified the arch, verify it is correct.");
        }

        let body = rt.block_on(response.bytes())?;
        file.write_all(&body)?;

        info!("[*] {} downloaded successfully!", &downloaded_file_path);

        fs::create_dir_all(&second_dir)?;
        
        // If filesystem is qcow2, extract accordingly, otherwise decompress the archive
        info!("[*] Decompressing {}...", &downloaded_file_path);
        if is_qcow2 {
            extract_qcow2(&downloaded_file_path, &second_dir)?;
        } else if is_gz {
            let tar_gz = File::open(&downloaded_file_path)?;
            let tar = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);
            archive.unpack(&second_dir)?;
        } else {
            // XZ decompression - similar to GZ but with XzDecoder
            let tar_xz = File::open(&downloaded_file_path)?;
            let tar = XzDecoder::new(tar_xz);
            let mut archive = Archive::new(tar);
            archive.unpack(&second_dir)?;
        }
    }

    // === For each file/directory in the user-supplied OS directory verify if it is present in the download, if file is not present then save file name to results ===
    let mut target_file: File;
    let mut digest: md5::Digest;
    let mut dl_target_file_hashes: HashMap<md5::Digest, String> = HashMap::new();

    for _file in WalkDir::new(&second_dir).follow_links(symlink).into_iter().filter_map(|file| file.ok()) {
        let file_path = _file.path();
        if _file.metadata().unwrap().is_file() {
            debug!("[*] DOWNLOAD FILE: {}", file_path.to_string_lossy().to_string());
            target_file = File::open(file_path)?;
            let mut bin_contents: Vec<u8> = Vec::new();
            target_file.read_to_end(&mut bin_contents)?;

            // Get the md5 of each file in the downloaded directory, add it to a dictionary with a name:hash keypair
            digest = md5::compute(bin_contents);
            dl_target_file_hashes.insert(digest, file_path.to_string_lossy().to_string());
            debug!("[*] DOWNLOAD FILE HASH: {:x}", digest);
        }
    }
    
    let mut usr_target_file_hashes: HashMap<md5::Digest, String> = HashMap::new();
    let usr_target = args[1].to_owned();
    
    for _file in WalkDir::new(usr_target).follow_links(symlink).into_iter().filter_map(|file| file.ok()) {
        let file_path = _file.path();
        if _file.metadata().unwrap().is_file() {
            debug!("[*] USER FILE: {}", file_path.to_string_lossy().to_string());
            target_file = File::open(file_path)?;
            let mut bin_contents: Vec<u8> = Vec::new();
            target_file.read_to_end(&mut bin_contents)?;

            // Get the md5 of each file in the downloaded directory, add it to a dictionary with a name:hash keypair
            digest = md5::compute(bin_contents);
            usr_target_file_hashes.insert(digest, file_path.to_string_lossy().to_string());
            debug!("[*] USER FILE HASH: {:x}", digest);
        }
    }

    let only_in_usr = find_unique_keys(&dl_target_file_hashes, &usr_target_file_hashes);
    
    if only_in_usr.is_empty() {
       println!("[!] No unique files were identified");
    } else {
       if !output_file_path.is_empty() {
            let mut output_file = File::create(&output_file_path)?;
            let results = only_in_usr.join("\n") + "\n";
            output_file.write_all(results.as_bytes())?;
            info!("[+] Results written to: {}", output_file_path);
       } else {
           println!("[+] Files which are only in the user provided dict: {:?}", only_in_usr);
       }
    }
    
    // if online download was done then clear artifacts
    if target2.is_empty() && !downloaded_file_path.is_empty() {
        info!("[*] Deleting artifacts {}...", downloaded_file_path);
        fs::remove_dir_all(&second_dir)?;
        fs::remove_file(&downloaded_file_path)?;
    }

    Ok(())
}

// TODO: Improve support for other distro filesystems