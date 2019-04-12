/*
right.. so, we need to run it against some project first
let's start with checking if it's a python project and running mypy?
it's a python prohect if:
1. .git is there, and there is a single subdir with __init__.py or we are the dir with __init__.py
*/
#[macro_use]
extern crate log;
extern crate simple_logger;
extern crate clap;
extern crate walkdir;
extern crate tempfile;


use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::fs;
use std::io::prelude::*;
use std::thread;
use tempfile::NamedTempFile;

use clap::{Arg, App};
use walkdir::{WalkDir, IntoIter, DirEntry};

type PathPredicate = Fn(&Path) -> RuciResult<bool>;

// TODO eh, the lifetime is a bit awkward... is that really necessary?
struct Interesting<'a> {
    iter: IntoIter,
    predicate: &'a PathPredicate,
}

struct FollowLinks {
    value: bool,
}

impl<'a> Interesting<'a> {
    fn walk(root: &Path, follow_links: FollowLinks, predicate: &'a PathPredicate) -> Self {
        Self {
            iter: WalkDir::new(root).follow_links(follow_links.value).into_iter(),
            predicate: predicate,
        }
    }
}


// original interesting: return dirs that are interesting; do not descend
// python targets: return all items conforming to is_py_file and modules (do not descend)
impl<'a> Iterator for Interesting<'a> {
    type Item = RuciResult<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.iter.next() {
                None => break,
                Some(Err(err)) => return Some(Err(err.to_string())),
                Some(Ok(entry)) => entry,
            };
            let ep = entry.path();
            let conforms = match (self.predicate)(ep) {
                Err(err) => return Some(Err(err)),
                Ok(conforms) => conforms,
            };
            if conforms {
                if entry.file_type().is_dir() {
                    self.iter.skip_current_dir();
                }
                return Some(Ok(PathBuf::from(ep)))
            }
        }
        None
    }
}



type RuciError = String;
type RuciResult<T> = Result<T, RuciError>;


fn is_ruci_target(path: &Path) -> RuciResult<bool> {
    if !path.is_dir() {// TODO FIXME do not swallow errors
        let ex = path.extension();
        if ex.map_or(false, |e| e == "py") {
            return Ok(true); // TODO eh. hacky
        } else {
            return Ok(false);
        }
    }
    if path.join(".noruci").exists() {
        return Ok(false);
        // TODO check my own git commits?
    }
    for pp in &[".git", ".ruci", "__init__.py"] {
        if path.join(pp).exists() {
            return Ok(true);
        }
    }
    return Ok(false);
}


fn is_ff(path: &Path, ext: &str, mimes: &[&str]) -> RuciResult<bool> {
    let ex = path.extension();
    if ex.map_or(false, |e| e == ext) { // TODO hmm. mimetype can do that..
        return Ok(true);
    }

    let meta = try!(fs::metadata(path).map_err(|_e| "error while retrieving meta!"));

    if meta.is_dir() {
        return Ok(false);
    }

    let mode = meta.permissions().mode();
    let exec_perms = 0o100;
    // if it's not executable, it can't be imported anyway.. so not checking
    if mode | exec_perms != mode {
        return Ok(false);
    }

    let ps = try!(path.to_str().ok_or("couldn't decode the path"));
    let res = Command::new("mimetype")
        .arg("-b")
        .arg("-L")
        .arg(ps)
        .output()
        .map_err(|_e| "io error!"); // TODO include error
    let out = try!(res);
    let stdout = try!(std::str::from_utf8(&out.stdout).map_err(|_e| "io error!"));
    let mime = stdout.replace("\n", "");
    return Ok(mimes.iter().find(|&&x| x == mime).is_some());
}


fn is_py_file(path: &Path) -> RuciResult<bool> {
    return is_ff(path, "py", &["text/x-python3", "text/x-python"]);
}


fn is_py_target(path: &Path) -> RuciResult<bool> {
    // TODO duplicate meta retrieval...
    let meta = try!(fs::metadata(path).map_err(|_e| "error while retrieving meta!"));
    if meta.is_dir() {
        // checks whether it is py module root
        // TODO FIXME eh. the results are sometimes different if you check as module vs checking as bunch of files
        // e.g. check on my module and my.coding
        let maybe_meta = fs::metadata(path.join("__init__.py"));
        let init_exists = maybe_meta.is_ok(); // TODO distinguish between errors??
        return Ok(init_exists);
    } else {
        return is_py_file(path);
    }
}


fn is_sh_file(path: &Path) -> RuciResult<bool> {
    return is_ff(path, "sh", &["application/x-shellscript"]);
}


fn is_dotgit(entry: &DirEntry) -> bool {
    entry.file_name()
        .to_str()
        .map(|s| s == ".git")
        .unwrap_or(false)
}


fn get_sh_targets(path: &Path) -> Vec<PathBuf> {
    // kinda overkilly way to skip .git dir..
    // https://github.com/BurntSushi/walkdir
    let walker = WalkDir::new(path).follow_links(true).into_iter();
    return walker.filter_entry(|e| !is_dotgit(e)).filter_map(
        |me| me.ok().map(|e| e.path().to_owned()).filter(|p| is_sh_file(p).unwrap_or(false))
    ).collect();
}

fn get_py_targets(path: &Path) -> RuciResult<Vec<PathBuf>> {
    let iter = Interesting::walk(path, FollowLinks{value: true}, &is_py_target).into_iter();
    return iter.collect();
}

fn check_mypy(path: &Path) -> RuciResult<()> {
    let targets = try!(get_py_targets(path));
    info!("\tmypy: {:?}: {:?}", path, targets);

    if targets.is_empty() {
        return Ok(());
    }

    // TODO how to handle io error?
    let mut cmd = Command::new("mypy");
    cmd.arg("--check-untyped-defs")
        .arg("--strict-optional")
        // .arg("--scripts-are-modules")
        .args(targets);
    debug!("{:?}", cmd);
    let res = cmd
        .output()
        .expect("failed to execute process"); // TODO wtf??
    // TODO not really panic, might not be worth terminating everything

    if !res.status.success() {
        return Err(format!("{}\n{}", String::from_utf8(res.stdout).unwrap(), String::from_utf8(res.stderr).unwrap()));
    }
    return Ok(()); // TODO meh..
}

fn check_pylint(path: &Path) -> RuciResult<()> {
    let targets = try!(get_py_targets(path));
    info!("\tpylint: {:?}: {:?}", path, targets);

    if targets.is_empty() {
        return Ok(());
    }

    // TODO ugh, pylint should run separately?
    // otherwise we get
    // /L/Dropbox/repos/scripts-new/wm/other-monitor: error: Duplicate module named '__main__'
    let res = try!(Command::new("pylint")  // TODO maybe, python3 -m pylint?
        .arg("-E")
        .args(targets)
        .output()
        .map_err(|e| format!("error while executing pylint {:?}", e)));
    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(());
}

fn check_pytest(path: &Path) -> RuciResult<()> {
    info!("\tpytest: {:?}", path);

    let mut file = try!(NamedTempFile::new().map_err(|e| format!("{:?}", e)));
    {
        let pytest_config = b"
[pytest]
python_files = '*.py'
";
        try!(file.write_all(pytest_config).map_err(|e| format!("{:?}", e)));
    }

    let res = try!(
        Command::new("pytest")
            .arg("-c")
            .arg(file.path())
            .arg(path)
            .output()
            .map_err(|e| format!("error while executing pytest {:?}", e))
    );
    let code = res.status.code();
    let out = String::from_utf8(res.stdout).unwrap();
    let err = String::from_utf8(res.stderr).unwrap();
    const NO_TESTS_COLLECTED: i32 = 5; // https://docs.pytest.org/en/latest/usage.html
    return code
        .ok_or(())
        .and_then(|ecode| if ecode == 0 || ecode == NO_TESTS_COLLECTED { Ok(()) } else { Err(()) })
        .map_err(|_| format!("pytest failed: {}\n{}", out, err));
}

fn check_shellcheck(path: &Path) -> RuciResult<()> {
    let targets = get_sh_targets(path);
    // TODO hmm, need to exlude .git directory?
    info!("\tshellcheck: {:?}: {:?}", path, targets);
    if targets.is_empty() {
        return Ok(());
    }

    let res = try!(Command::new("shellcheck")
                   .args(targets)
                   .output()
                   .map_err(|e| format!("error while executing shellecheck {:?}", e)));
    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(())
}

fn check_dir(path: &Path, with_pytest: bool) -> RuciResult<()> {
    let pc = path.to_owned();
    let mypy = thread::spawn(move || {
        return check_mypy(&pc);
    });

    // TODO really, this is how it's done?..
    let pc2 = path.to_owned();
    let pylint = thread::spawn(move || {
        return check_pylint(&pc2);
    });

    let sc = path.to_owned();
    let shellcheck = thread::spawn(move || {
        return check_shellcheck(&sc);
    });

    let tc = path.to_owned();
    let pytest = thread::spawn(move || {
        return check_pytest(&tc);
    });


    let checks = {
        let mut res = vec![
            mypy,
            pylint,
            shellcheck,
        ];
        if with_pytest {
            res.push(pytest);
        }
        res
    };

    let mut chresults = vec![];
    for c in checks {
        let res = c.join();
        chresults.push(res.unwrap()); // TODO handle errors?
    }

    // TODO err, why into_iter works for vector but not for array?
    // ah! iter() is always borrowing?
    let out: Vec<String> = chresults.into_iter().filter_map(|thing: RuciResult<()>| {
        match thing {
            Ok(_)  => Option::None,
            Err(e) => Option::from(e),
        }
    }).collect();
    if out.len() == 0 {
        return Ok(())
    } else {
        return Err(out.join("\n"))
    }
}

// TODO I guess we could have two modes
// One mode is running against the whole filesystem; then we look for .ruci files? Since checking everything is pretty unrealistic
// Another mode is running against pre

fn main() {
    simple_logger::init().unwrap();

    let matches = App::new("RuCi")
                        .about("Quickchecks stuff")
                        .arg(Arg::with_name("test").long("test"))
                        .arg(Arg::with_name("path")
                            // .long("path")
                            // .short("p")
                            .multiple(true)
                            // .index(1)
                            // .value_name("path")
                             // .takes_value(true)
                        )
                        .get_matches();

    let with_pytest = matches.is_present("test");

    // Gets a value for config if supplied by user, or defaults to "default.conf"
    let paths = matches.values_of("path"); // TODO or current dir??_or(["."]);

    let targets: Vec<_> = paths.map(|ps| ps.collect()).unwrap_or(vec!["."]);

    // println!("{:?}", &targets);

    /*
       eh, ok, this looks a bit meh.
       on the one hand, we wanna output stuff ASAP during processing, just a nice thing to do
       on the other hand, logging is not badly mutable and it's nice to implement this without explcit for loop
    */
    // TODO shit order is not deterministic because of walkdir...
    let errors: Vec<_> = targets.iter()
        .flat_map(|ps| Interesting::walk(&fs::canonicalize(Path::new(ps)).unwrap(), FollowLinks{value: false}, &is_ruci_target))
        .filter_map(|target| {
            let target = match target {
                Err(err) => return Option::from(Err(err)),
                Ok(target) => target,
            };
            info!("checking {:?}", target);
            let res = check_dir(&target, with_pytest);
            match &res {
                Ok(_) => info!("... success"),
                Err(e) => error!("... ERROR\n{}", e),
            }
            return Option::from(res);
    }).filter_map(Result::err).collect();

    for e in &errors {
        error!("ERROR:\n\t{}", e);
    }

    exit(if errors.is_empty() {0} else {1});
}
// TODO cache .. maybe run mypy while updating cache?

// TODO maybe it could also generate json report? Then we could track when something broke.. and notify depending on importance (e.g. kython or kron are pretty important)
