mod config;

/*
right.. so, we need to run it against some project first
let's start with checking if it's a python project and running mypy?
it's a python prohect if:
1. .git is there, and there is a single subdir with __init__.py or we are the dir with __init__.py
*/
#[macro_use]
extern crate log;
extern crate simple_logger;

use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::fs;

fn get_py_module_root(p: &Path) -> PathBuf {
    fn has_init(p: &Path) -> bool {
        return p.join("__init__.py").exists();
    }
    if has_init(p) {
        return PathBuf::from(p);
    }
    let mut filtered: Vec<_> = fs::read_dir(p).unwrap().filter_map(|d| {
        // TODO why as_ref here???
        let pp = d.as_ref().unwrap().path();
        if pp.is_dir() && has_init(&pp) {
            Option::from(pp)
        } else {
            Option::None
        }
    }).collect();
    // let () = filtered;
    if filtered.len() != 1 {
        panic!("{:?}", filtered);
    }
    // TODO err, pretty ugly
    return filtered.remove(0);
}

// TODO return result?
fn check_dir(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("Path {:?} is not a directory! ERROR!", path));
    }
    if !path.join(".git").exists() {
        return Err(String::from("no .git directory... skipping"));
    }
    let py_module = get_py_module_root(path);
    // TODO how to handle io error?
    // TODO collect all errors?
    let res = Command::new("mypy")
        .arg(py_module)
        .output()
        .expect("failed to execute process"); // TODO is it really panic?
    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(()); // TODO err really?
    // println!("{:?}", );
}

fn main() {
    simple_logger::init().unwrap();

    /*
       eh, ok, this looks a bit meh.
       on the one hand, we wanna output stuff ASAP during processing, just a nice thing to do
       on the other hand, logging is not badly mutable and it's nice to implement this without explcit for loop
    */
    let errors: Vec<_> = config::TARGETS.iter().map(|target| {
        info!("Checking {:?}", target);
        let res = check_dir(Path::new(target));
        match &res {
            Ok(_) => info!("... succcess!"),
            Err(e) => error!("{}", e),
        }
        res
    }).filter(Result::is_err).collect();

    exit(if errors.is_empty() {0} else {1});
}
