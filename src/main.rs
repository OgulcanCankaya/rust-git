extern crate core;
use core::borrow::Borrow;
use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{Cred, FetchOptions,MergeOptions, RemoteCallbacks,Commit, ObjectType};
use git2::{Oid, Signature,Direction,Repository,Progress};
use std::cell::RefCell;
use std::fs::{File};
use std::io::{self, Write};
use std::iter::IntoIterator;
use std::path::{Path,PathBuf};
use std::process::Command;
use std::{thread, time};
use yaml_rust::{YamlLoader};
use std::io::Read;
use std::collections::HashMap;
use clap::{Arg, App, SubCommand};
use clap::AppSettings;
use itertools::Itertools;
extern crate yaml_rust;

//#[derive(Deserialize)]
struct State {
    progress: Option<Progress<'static>>,
    total: usize,
    current: usize,
    path: Option<PathBuf>,
    newline: bool,
}

fn print(state: &mut State) {
    let stats = state.progress.as_ref().unwrap();
    let network_pct = (100 * stats.received_objects()) / stats.total_objects();
    let index_pct = (100 * stats.indexed_objects()) / stats.total_objects();
    let co_pct = if state.total > 0 {
        (100 * state.current) / state.total
    } else {
        0
    };
    let kbytes = stats.received_bytes() / 1024;
    if stats.received_objects() == stats.total_objects() {
        if !state.newline {
            println!();
            state.newline = true;
        }
        print!(
            "Resolving deltas {}/{}\r",
            stats.indexed_deltas(),
            stats.total_deltas()
        );
    } else {
        print!(
            "net {:3}% ({:4} kb, {:5}/{:5})  /  idx {:3}% ({:5}/{:5})  \
             /  chk {:3}% ({:4}/{:4}) {}\r",
            network_pct,
            kbytes,
            stats.received_objects(),
            stats.total_objects(),
            index_pct,
            stats.indexed_objects(),
            stats.total_objects(),
            co_pct,
            state.current,
            state.total,
            state
                .path
                .as_ref()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    }
    io::stdout().flush().unwrap();
}
fn clone(url : &String, path : &String,ssh_priv : &String, ssh_public : &String) -> Result<(), git2::Error> {
    println!("url  {}\npath  {}",url.clone(),Path::new(path).display());

//    let state = RefCell::new(State {
//        progress: None,
//        total: 0,
//        current: 0,
//        path: None,
//        newline: false,
//    });
    let mut cb = RemoteCallbacks::new();
//    cb.transfer_progress(|stats| {
//        let mut state = state.borrow_mut();
//        state.progress = Some(stats.to_owned());
//        print(&mut *state);
//        true
//    });
    cb.credentials(|_, _, _| {
        let creds = Cred::ssh_key(
            "git",
            Some(Path::new(ssh_public)),
            Path::new(ssh_priv),
            None
        ).expect("Could not create credentials object.");
        Ok(creds)
    });
    let mut co = CheckoutBuilder::new();
//    co.progress(|path, cur, total| {
//        let mut state = state.borrow_mut();
//        state.path = path.map(|p| p.to_path_buf());
//        state.current = cur;
//        state.total = total;
//        print(&mut *state);
//    });
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(cb);
    RepoBuilder::new()
        .fetch_options(fo)
        .with_checkout(co)
        .clone(url, Path::new(path))?;
    println!();
    Ok(())
}
fn find_last_commit(repo: &Repository) -> Result<Commit, git2::Error> {
    let obj = repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|_| git2::Error::from_str("Couldn't find commit"))
}
fn display_commit(commit: &Commit) { /*proof of work :) */
    println!("display parent commit");
    println!("commit {}\nAuthor: {}\nDate:   \n    {}",
             commit.id(),
             commit.author(),
             commit.message().unwrap_or("no commit message"));
}
fn add_and_commit(repo: &Repository, message: &str , sign_name : &String , sign_mail : &String) -> Result<Oid,git2::Error> {
    let parent_commit = find_last_commit(&repo)?;
    let mut index  = repo.index().expect("index error");
    index.add_all(vec!["."].iter(),git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let oid = index.write_tree().expect("write tree");
    let signature = Signature::now(&sign_name, &sign_mail).expect("sign");
    let tree = repo.find_tree(oid).expect("find tree");
    repo.commit(Some("HEAD"), //  point HEAD to our new commit
                &signature, // author
                &signature, // committer
                message, // commit message
                &tree, // tree
                &[&parent_commit]) // parents
}
fn fetch(path: &Path, ssh_priv : &String, ssh_public : &String)  -> Result<(), git2::Error>  {
    let state = RefCell::new(State {
        progress: None,
        total: 0,
        current: 0,
        path: None,
        newline: false,
    });
    let repo =  Repository::open(path).expect("open repo error");
    let mut cb = RemoteCallbacks::new();
    let mut remote = repo.find_remote("origin").expect("find remote error");
    cb.update_tips(|refname, a, b| {
        if a.is_zero() {
            println!("[new]     {:20} {}", b, refname);
        } else {
            println!("[updated] {:10}..{:10} {}", a, b, refname);
        }
        true
    });
    cb.transfer_progress(|stats| {
        let mut state = state.borrow_mut();
        state.progress = Some(stats.to_owned());
        print(&mut *state);
        true
    });
    let mut co = CheckoutBuilder::new();
    co.progress(|path, cur, total| {
        let mut state = state.borrow_mut();
        state.path = path.map(|p| p.to_path_buf());
        state.current = cur;
        state.total = total;
//        print(&mut *state);
    });
    cb.credentials(|_, _, _| {
        let creds = Cred::ssh_key(
            "git",
            Some(Path::new(&ssh_public)),
            Path::new(&ssh_priv),
            None
        ).expect("Could not create credentials object.");
        Ok(creds)
    });
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(cb);
    remote.download(&[], Some(&mut fo)).expect("remote.download error");
    { let stats = remote.stats();
        if stats.local_objects() > 0 {
            println!(
                "\rReceived {}/{} objects in {} bytes (used {} local \
                 objects)",
                stats.indexed_objects(),
                stats.total_objects(),
                stats.received_bytes(),
                stats.local_objects()
            );
        } else {
            println!(
                "\rReceived {}/{} objects in {} bytes",
                stats.indexed_objects(),
                stats.total_objects(),
                stats.received_bytes()
            );
        }
    }
    remote.disconnect();
    remote.update_tips(None, true, git2::AutotagOption::All, None).expect("update tips error");
    Ok(())
}
fn merge(path: &Path,sign_name : &String , sign_mail : &String) -> Result<(), git2::Error> {
    let repo = Repository::open(path)?;
    if repo.index().unwrap().has_conflicts() == false {
        add_and_commit(&repo,"merging origin/master to master",sign_name  , sign_mail )?;
    }
    if repo.index().unwrap().has_conflicts() == true {
        println!("{} index has conflicts. Resolve them first. Merge failed...",repo.path().display());
        return Ok(())
    }
    let reference = repo.find_reference("FETCH_HEAD")?;
    let fetch = repo.reference_to_annotated_commit(&reference)?;
    let oid = repo.refname_to_id("refs/remotes/origin/master")?;
    let mut co = CheckoutBuilder::new();
    let mo = &mut MergeOptions::new();
    repo.merge(&[&fetch] ,Some(mo),Some(&mut co))?;
    repo.cleanup_state()?;
    let object = repo.find_object(oid, None).unwrap();
    repo.reset(&object, git2::ResetType::Hard, None)?; /*resets repo to origin/master ??? */
    Ok(())
}
fn pull (path :& Path,sign_name : &String , sign_mail : &String,ssh_priv : &String , ssh_pub : &String){
    fetch(path,ssh_priv,ssh_pub).expect("fetch of pull failed");
    merge(path, &sign_name, &sign_mail).expect("merge of pull failed");
}
fn  multi_pull(dirs : & Vec<String>,sign_name : &String , sign_mail : &String, ssh_priv : &String , ssh_pub : &String){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        pull(path,&sign_name,&sign_mail,&ssh_priv,&ssh_pub);
    }
    println!();
    wait(2);
}
fn push (path: &Path, ssh_priv : &String , ssh_pub : &String) -> Result<(), git2::Error>  {
    let repo = Repository::open(path).expect("push op. open repository error");
    let mut remote = repo.find_remote("origin").expect("push op. find remote origin");
    let mut cb = RemoteCallbacks::new();
    cb.credentials(|_, _, _| {
        let creds = Cred::ssh_key(
            "git",
            Some(Path::new(&ssh_pub)),
            Path::new(&ssh_priv),
            None
        ).expect("Could not create credentials object.");
        Ok(creds)
    });
    let mut po = git2::PushOptions::new();
    po.remote_callbacks(cb);
    /*problem here*/
    remote.connect(Direction::Push)?;
    /*aaand ends here */  /*idk why but it is gone*/
    /*
    if haconflicts return err
    */
    remote.push(&["refs/heads/master:refs/heads/master"], Some(& mut po))
}
fn  multi_push(dirs : & Vec<String>,ssh_priv : &String , ssh_pub : &String){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        push(path,&ssh_priv,&ssh_pub);
    }
    println!();
    wait(4);
}
fn print_long(statuses: &git2::Statuses) {
    let mut header = false;
    let mut rm_in_workdir = false;
    let mut changes_in_index = false;
    let mut changed_in_workdir = false;

    // Print index changes
    for entry in statuses
        .iter()
        .filter(|e| e.status() != git2::Status::CURRENT)
        {
            if entry.status().contains(git2::Status::WT_DELETED) {
                rm_in_workdir = true;
            }
            let istatus = match entry.status() {
                s if s.contains(git2::Status::INDEX_NEW) => "new file: ",
                s if s.contains(git2::Status::INDEX_MODIFIED) => "modified: ",
                s if s.contains(git2::Status::INDEX_DELETED) => "deleted: ",
                s if s.contains(git2::Status::INDEX_RENAMED) => "renamed: ",
                s if s.contains(git2::Status::INDEX_TYPECHANGE) => "typechange:",
                _ => continue,
            };
            if !header {
                println!(
                    "\
# Changes to be committed:
#   (use \"git reset HEAD <file>...\" to unstage)
#"
                );
                header = true;
            }

            let old_path = entry.head_to_index().unwrap().old_file().path();
            let new_path = entry.head_to_index().unwrap().new_file().path();
            match (old_path, new_path) {
                (Some(old), Some(new)) if old != new => {
                    println!("#\t{}  {} -> {}", istatus, old.display(), new.display());
                }
                (old, new) => {
                    println!("#\t{}  {}", istatus, old.or(new).unwrap().display());
                }
            }
        }

    if header {
        changes_in_index = true;
        println!("#");
    }
    header = false;

    // Print workdir changes to tracked files
    for entry in statuses.iter() {
        // With `Status::OPT_INCLUDE_UNMODIFIED` (not used in this example)
        // `index_to_workdir` may not be `None` even if there are no differences,
        // in which case it will be a `Delta::Unmodified`.
        if entry.status() == git2::Status::CURRENT || entry.index_to_workdir().is_none() {
            continue;
        }

        let istatus = match entry.status() {
            s if s.contains(git2::Status::WT_MODIFIED) => "modified: ",
            s if s.contains(git2::Status::WT_DELETED) => "deleted: ",
            s if s.contains(git2::Status::WT_RENAMED) => "renamed: ",
            s if s.contains(git2::Status::WT_TYPECHANGE) => "typechange:",
            _ => continue,
        };

        if !header {
            println!(
                "\
# Changes not staged for commit:
#   (use \"git add{} <file>...\" to update what will be committed)
#   (use \"git checkout -- <file>...\" to discard changes in working directory)
#\
                ",
                if rm_in_workdir { "/rm" } else { "" }
            );
            header = true;
        }

        let old_path = entry.index_to_workdir().unwrap().old_file().path();
        let new_path = entry.index_to_workdir().unwrap().new_file().path();
        match (old_path, new_path) {
            (Some(old), Some(new)) if old != new => {
                println!("#\t{}  {} -> {}", istatus, old.display(), new.display());
            }
            (old, new) => {
                println!("#\t{}  {}", istatus, old.or(new).unwrap().display());
            }
        }
    }

    if header {
        changed_in_workdir = true;
        println!("#");
    }
    header = false;

    // Print untracked files
    for entry in statuses
        .iter()
        .filter(|e| e.status() == git2::Status::WT_NEW)
        {
            if !header {
                println!(
                    "\
# Untracked files
#   (use \"git add <file>...\" to include in what will be committed)
#"
                );
                header = true;
            }
            let file = entry.index_to_workdir().unwrap().old_file().path().unwrap();
            println!("#\t{}", file.display());
        }
    header = false;

    // Print ignored files
    for entry in statuses
        .iter()
        .filter(|e| e.status() == git2::Status::IGNORED)
        {
            if !header {
                println!(
                    "\
# Ignored files
#   (use \"git add -f <file>...\" to include in what will be committed)
#"
                );
                header = true;
            }
            let file = entry.index_to_workdir().unwrap().old_file().path().unwrap();
            println!("#\t{}", file.display());
        }

    if !changes_in_index && changed_in_workdir {
        println!(
            "no changes added to commit (use \"git add\" and/or \
             \"git commit -a\")"
        );
    }
}
fn status(path : &Path){
    let repo = Repository::open(path).expect("repo opening error");
    let mut so = git2::StatusOptions::new();
    let statuses = repo.statuses(Some(&mut so)).expect("stat take error");
    print_long(statuses.borrow());
}
fn multi_commit(dirs : & Vec<String>,sign_name : &String , sign_mail : &String){
    println!("Please enter global commit message..");
    let mut com_msg = String::new();
    std::io::stdin().read_line(&mut com_msg);
    com_msg=com_msg.trim().to_string();

    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        let mut repo = Repository::open(path).expect("hede");
        add_and_commit(&repo,&com_msg,&sign_name,&sign_mail);
    }
}
fn  multi_status(dirs : & Vec<String>){
    println!("Statuses of repositories are...\n");
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        status(path);
    }
    println!();
    println!("If there is no explaining for repositories. It is probably clean and up-to-date...");  /*adam akıllı kouş lan*/
    println!("You may check. Just to be sure...");
    println!();
    wait(4);
}
fn  multi_merge(dirs : & Vec<String> , sign_name : &String , sign_mail : &String){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        merge(path, &sign_name  , &sign_mail ).expect("merge failed");
    }
    println!();
    wait(4);
}
fn  multi_fetch(dirs : & Vec<String>, ssh_priv : &String, ssh_public : &String){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        fetch(path,ssh_priv,ssh_public);
    }
    println!();
    wait(3)
}
fn run_command(command_name: &String, commnd : &Vec<String>, path : &String){
//    println!("{}",path.clone());
    let mut command = Command::new(command_name).current_dir(path.clone().to_string()).args(commnd.into_iter()).output().expect("command failed to start");       /*curent directory operation*/
    io::stdout().write_all(&command.stdout).unwrap();
}
fn wait(seconds: u64){
    let ten_millis = time::Duration::from_secs(seconds);
    let now = time::Instant::now();
    thread::sleep(ten_millis);
    assert!(now.elapsed() >= ten_millis);
}

fn main() {
    let mut f = File::open("conf.yaml").unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s);
    let configs = YamlLoader::load_from_str(&s).unwrap();
    let conf = &configs[0];
    let mut repo_dirs = HashMap::new();
    for (i,j) in conf["repos"].clone().into_hash().expect("dfs").iter(){
        repo_dirs.insert(i.as_str().expect("dvssdf").to_string(),j.as_str().expect("dvssdf").to_string());
    }

    let mut repo_list : HashMap<String,Vec<String>>  = HashMap::new();
    for (i,j) in conf["lists"].clone().into_hash().expect("dfs").iter(){
        let mut temp_repo_conta : Vec<String>= Vec::new();
        for k in j.clone() {
//            println!("{:?}",k.as_str().unwrap());
            temp_repo_conta.push(k.as_str().unwrap().to_string());
        }
        repo_list.insert(i.as_str().expect("sdfsf").to_string(),temp_repo_conta.clone());
    }


//    for i in repo_list.get("nlist").unwrap() {
//        println!("{:?}",i);
//    }
//    println!("{:?}",repo_list);





    println!("{:#?}", repo_dirs);
    let mut glob_ssh_priv = String::new();
    let mut glob_ssh_pub = String::new();
    let mut glob_sign_mail = String::new();
    let mut glob_sign_name = String::new();
    glob_ssh_priv = conf["ssh_priv"].clone().as_str().expect("no ssh private key").to_string();
    glob_ssh_pub = conf["ssh_pub"].clone().as_str().expect("no ssh private key").to_string();
    glob_sign_name=conf["signature_name"].clone().as_str().expect("no signature name found").to_string();
    glob_sign_mail=conf["signature_mail"].clone().as_str().expect("no signature mail found").to_string();
    let credentials = Cred::ssh_key(
        "git",
        Some(Path::new(&glob_ssh_pub)), /*other functions take this from config maker. disable it*/
        Path::new(&glob_ssh_priv),
        None
    ).expect("Could not create credentials object.");





    let matches = App::new("rustit")
        .version("0.1")
        .author("catastrophe <hede-hodo@wtf.com>")
        .about("alayına isyan")
        .subcommand(SubCommand::with_name("list")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .default_value("all")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("status")
            .about("controls status features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("merge")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("pull")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("clone")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("push")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1)))

        .subcommand(SubCommand::with_name("fetch")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true).default_value("all")
                .takes_value(true).min_values(1)))

        .subcommand(SubCommand::with_name("exec")
            .about("controls testing features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .setting(AppSettings::TrailingVarArg)
            .arg(Arg::with_name("repos")
                .help("enter repos").multiple(true)
                .takes_value(true).required(true).min_values(1))
            .arg(Arg::with_name("command")
                .help("print debug information verbosely").last(true)
                .takes_value(true).required(true).min_values(1)))
        
        .subcommand(SubCommand::with_name("clone")
            .about("controls cloning features")
            .version("1.3")
            .author("Someone E. <someone_else@other.com>")
            .arg(Arg::with_name("url_path")
                .help("enter repos").multiple(true).default_value("all")
                .takes_value(true).min_values(1)))
        .get_matches();


        /*checking what are you doing*/
    if matches.is_present("fetch") {
        println!("wanted to fetch");
        let mut aa =matches.subcommand_matches("fetch").expect("hede")
            .values_of("repos").expect("hodo");
        println!("{:?}",aa);
        for i in aa {
            println!("{}",i)
        }
    }
    if matches.is_present("list") {
        println!("wanted to list");
        let mut repos =matches.subcommand_matches("list").expect("hede")
                .values_of("repos").expect("hodo");
        let mut repo_vec:Vec<String> =Vec::new();
        for i in repos.clone() {
            repo_vec.push(i.to_string());
        }
        for i in repo_vec.clone() {
            if repo_list.contains_key(&i) {
                let mut ind : usize = 0;
                for k in repo_vec.clone() {
                    if k == i { break;}
                    ind = ind + 1;
                }
                println!("deleting {}",repo_vec.get(ind).unwrap().to_string());
                repo_vec.remove(ind);
                println!("found a list : {}", i.to_string());
                for j in repo_list.get(&i.to_string()).unwrap() {
                    repo_vec.push(j.to_string());
                }
            }
        }
        let mut repo_vec_unique :Vec<_> = repo_vec.clone().into_iter().unique().collect();

        if repo_vec.contains(&"all".to_string()) {
            for i in repo_dirs.clone().keys() {
                repo_vec_unique.push(i.to_string());
            }
        }




        for i in repo_vec_unique.clone() {
            if !repo_dirs.contains_key(&i) {
                println!("\nNo repository found with name {}\n-----------------------------------",i);
                continue;
            }
            println!("Path for {} is :  \n{}\n-----------------------------------",i,repo_dirs.get(&i).unwrap().to_string());
        }
    }




    if matches.is_present("exec") {
        println!("wanted to exec");
        let mut aa = matches.subcommand_matches("exec").expect("hede");
        let mut cmm = aa.values_of("command").expect("hodo");
        let mut repos = aa.values_of("repos").expect("lalalolo");
        let mut cmnd_vec:Vec<String> =Vec::new();
        let mut repo_vec:Vec<String> =Vec::new();
        for i in cmm.clone() {
            cmnd_vec.push(i.to_string());
        }
        for i in repos.clone() {
            repo_vec.push(i.to_string());
        }
        /*Deleting repo-list names from repo names so there would be no None type value*/
        for i in repo_vec.clone() {
            if repo_list.contains_key(&i) {
                let mut ind : usize = 0;
                for k in repo_vec.clone() {
                    if k == i { break;}
                    ind = ind + 1;
                }
                println!("deleting {}",repo_vec.get(ind).unwrap().to_string());
                repo_vec.remove(ind);
                println!("found a list : {}", i.to_string());
                for j in repo_list.get(&i.to_string()).unwrap() {
                    println!("these are the contents{:?}",i);
                    repo_vec.push(j.to_string());
                }
            }
        }
        /*deleting unique values*/
        let mut repo_vec_unique :Vec<_> = repo_vec.clone().into_iter().unique().collect();
        let mut cmd_name = String::new();
        cmd_name=cmnd_vec[0].clone().to_string();
        println!("{}",cmd_name);
        cmnd_vec.remove(0);
        println!("{:#?}",repo_dirs);
        println!("{:#?}",repo_vec_unique);
        for i in repo_vec_unique {
            println!("{:?}",&repo_dirs.clone().get(i.as_str()));
            run_command(&cmd_name,&cmnd_vec,&repo_dirs.clone().get(&i).unwrap().to_string());
        }
    }
    if matches.is_present("merge") {
        println!("wanted to merge");
        let mut aa =matches.subcommand_matches("merge").expect("hede")
            .values_of("command").expect("hodo");
        println!("{:?}",aa);
        println!("{:?}",matches.subcommand_matches("merge").expect("lolo").values_of("repos").expect("lala"));
    }
    if matches.is_present("status") {
        println!("wanted to status");
        let mut aa =matches.subcommand_matches("status").expect("hede")
            .values_of("command").expect("hodo");
        println!("{:?}",aa);
        println!("{:?}",matches.subcommand_matches("status").expect("lolo").values_of("repos").expect("lala"));
    }
    if matches.is_present("clone") {
        println!("wanted to merge");
        let mut aa =matches.subcommand_matches("clone").expect("hede")
            .values_of("url_path").expect("hodo");
        println!("{:?}",aa);
        println!("{:?}",matches.subcommand_matches("clone").expect("lolo").values_of("url_path").expect("lala"));
    }
    /*goodbye message*/

}