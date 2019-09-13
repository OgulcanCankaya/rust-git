#[deny(warnings)]
extern crate core;
//use docopt::Docopt;
use core::borrow::Borrow;
use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{Cred, FetchOptions,MergeOptions, RemoteCallbacks,Commit, ObjectType};
use git2::{Oid, Signature,Direction,Repository,Progress};
use serde::{Deserialize,Serialize};
use std::cell::RefCell;
use std::fs::{self,File};
use std::io::{self, Write};
use std::iter::IntoIterator;
use std::path::{Path,PathBuf};
use std::process::Command;
use std::{thread, time};

//#[derive(Deserialize)]
struct State {
    progress: Option<Progress<'static>>,
    total: usize,
    current: usize,
    path: Option<PathBuf>,
    newline: bool,
}
//fn str_concat( x: &String ,  y:  &String) -> String {
//    return &y.clone().c &x.clone();
//}
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
fn clone(url : &String, path : &String,config : &Config) -> Result<(), git2::Error> {
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
            Some(Path::new(&config.ssh_pub)),
            Path::new(&config.ssh_priv),
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
fn add_and_commit(repo: &Repository, message: &str) -> Result<Oid,git2::Error> {
    let mut config = config_maker();
    let parent_commit = find_last_commit(&repo)?;
    let mut index  = repo.index().expect("index error");
    index.add_all(vec!["."].iter(),git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let oid = index.write_tree().expect("write tree");
    let signature = Signature::now(&config.signature_name, &config.signature_mail).expect("sign");
    let tree = repo.find_tree(oid).expect("find tree");
    repo.commit(Some("HEAD"), //  point HEAD to our new commit
                &signature, // author
                &signature, // committer
                message, // commit message
                &tree, // tree
                &[&parent_commit]) // parents
}
fn fetch(path: &Path, config: Config)  -> Result<(), git2::Error>  {
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
            Some(Path::new(&config.ssh_pub)),
            Path::new(&config.ssh_priv),
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
fn merge(path: &Path,config :Config) -> Result<(), git2::Error> {
    let repo = Repository::open(path)?;
    if repo.index().unwrap().has_conflicts() == true {
        add_and_commit(&repo,"merging origin/master to master")?;
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
fn pull (path :& Path){
    let cfg1 = config_maker();
    let cfg2 = config_maker();
    merge(path, cfg1);
    fetch(path, cfg2);
}

fn  multi_pull(dirs : & Vec<String>){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        pull(path);
    }
    println!();
    wait(4);
}


fn push (path: &Path, config : Config ) -> Result<(), git2::Error>  {
    let repo = Repository::open(path).expect("push op. open repository error");
    let mut remote = repo.find_remote("origin").expect("push op. find remote origin");
    let mut cb = RemoteCallbacks::new();
    cb.credentials(|_, _, _| {
        let creds = Cred::ssh_key(
            "git",
            Some(Path::new(&config.ssh_pub)),
            Path::new(&config.ssh_priv),
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
fn  multi_push(dirs : & Vec<String>){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        push(path,config_maker());
    }
    println!();
    wait(4);
}
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    repo_parent: Vec<String>,
    ssh_pub: String,
    ssh_priv: String,
    signature_name: String,
    signature_mail: String,
}
fn config_maker() -> Config {
    if !Path::new("rustit.yaml").exists() {
        println!("\
                # Since you do not have a rustit configuration file,\
                    we are assuming you are using this for the first time."
        );
        println!("Couldn't fnd rustit.yaml file. Creating one...");
        let mut repo_par : Vec<String> = Vec::new();
        let mut ssh_public = String::new();
        let mut ssh_private = String::new();
        let mut sign_name = String::new();
        let mut sign_mail = String::new();
        println!("Please enter user name for commit Signatures:  ");
        std::io::stdin().read_line(& mut sign_name).unwrap();
        println!("Please enter user mail for commit Signatures:  ");
        std::io::stdin().read_line(& mut sign_mail).unwrap();
        println!("Please enter private ssh key path in absolute path format. \nEq. /home/user/.ssh/id_rsa :  ");
        std::io::stdin().read_line(& mut ssh_private).unwrap();
        println!("Please enter public ssh key path in absolute path format. \nEq. /home/user/.ssh/id_rsa.pub :  ");
        std::io::stdin().read_line(& mut ssh_public).unwrap();
        println!("Please be careful at this state. You will enter your \"Parent\" repo directories. \nIf your repos' path is /home/user/Desktop/repos/repo1, please enter /home/user/Desktop/repos.");
        loop {
            let mut temp = String::new();
            println!("Please enter q to end this step or enter your repos' parent paths...");
            std::io::stdin().read_line(&mut temp).unwrap();
            if &temp == "q\n" {
                break;
            };
            repo_par.push(temp.trim().to_string());
        }
        sign_name = sign_name.trim().to_string();
        sign_mail = sign_mail.trim().to_string();
        ssh_private = ssh_private.trim().to_string();
        ssh_public = ssh_public.trim().to_string();
        let mut cfg  = Config { repo_parent: repo_par, ssh_pub: ssh_public, ssh_priv: ssh_private, signature_name: sign_name, signature_mail: sign_mail };
        println!("{:?}",cfg);
        let file = File::create("rustit.yaml").expect("rustit.yaml (conf file) creation failed...");
        let s = serde_yaml::to_string(&cfg).expect("to str failed");
        println!("s is ::  {:?}",s);
        fs::write(Path::new("rustit.yaml"),s).expect("write error came up");
    }
    let f = File::open("rustit.yaml").unwrap();
    let config: Config = serde_yaml::from_reader(f).unwrap();
    return config
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

fn multi_commit(dirs : & Vec<String>){
    println!("Please enter global commit message..");
    let mut com_msg = String::new();
    std::io::stdin().read_line(&mut com_msg);
    com_msg=com_msg.trim().to_string();

    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        let mut repo = Repository::open(path).expect("hede");
        add_and_commit(&repo,&com_msg);
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

fn  multi_merge(dirs : & Vec<String>,cof: Config){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        merge(path,config_maker());
    }
    println!();
    wait(4);
}

fn  multi_fetch(dirs : & Vec<String>,cof: Config){
    for dir in dirs {
        let mut path = Path::new(&dir);
        println!( "Repository path : {} ..." , path.display());
        fetch(path,config_maker());
    }
    println!();
    wait(3)
}

fn run_command(command_name: &String, commnd : &Vec<String>, path : &String){
    println!("{}",path.clone());
    let mut command = Command::new(command_name).args(commnd.into_iter()).output().expect("command failed to start");       /*curent directory operation*/
    io::stdout().write_all(&command.stdout).unwrap();
}

fn wait(seconds: u64){
    let ten_millis = time::Duration::from_secs(seconds);
    let now = time::Instant::now();
    thread::sleep(ten_millis);
    assert!(now.elapsed() >= ten_millis);
}

fn main() {
    let mut config: Config = config_maker();
    let mut repo_dirs: Vec<String> = Vec::new();
    for fileList in config.repo_parent.iter() {
        for entry in fs::read_dir(Path::new(fileList)).expect("Unable to list") {
            let entry = entry.expect("unable to get entry");
            let sub_dir = match Repository::open(entry.path()){
                Ok(Repository) =>  repo_dirs.push(entry.path().to_str().unwrap().to_string()),
                Err(e ) => (),
            };
        }
    }

    println!("Your current configuration: \n\nRepo_parent(s): {:#?}",config.repo_parent);
    println!("Your SSH key pairs\n\tPrivate: {}\n\tPublic: {}\nSignature name and email: {} - {}",config.ssh_priv,config.ssh_pub,config.signature_name,config.signature_mail);
    print!("If there is a problem with your configuration please try to edit rustit.yaml file in default format...\n");
    println!("\n");
    println!("You currently have {} repositories.", repo_dirs.len());
    let mut tmp = String::new();
    print!("Do you want to see their names ? [y/n] :");
    println!();
    std::io::stdin().read_line(&mut tmp);
    if tmp =="y\n" {
        for i in &repo_dirs {
            println!("{}", i);
        }
    }
    println!();
    let credentials = Cred::ssh_key(
        "git",
        Some(Path::new(&config.ssh_pub)),
        Path::new(&config.ssh_priv),
        None
    ).expect("Could not create credentials object.");
    loop {
        let mut temp = String::new();
        println!("Please select the operation you want to do.\n1.Status\n2.Fetch\n3.Merge\n4.Pull\n5.Push\n6.Clone\n7.Commit\n8.Custom command");
        println!(": ");
        std::io::stdin().read_line(&mut temp).unwrap();
        if &temp == "q\n" {
            break;
        };
        if &temp == "1\n" {
            /*status  */
            multi_status(&repo_dirs);
        };
        if &temp == "2\n" {
            /*fetch*/
            multi_fetch(&repo_dirs,config_maker());
        };
        if &temp == "3\n" {
            /*Merge*/
            println!("This operation works as \"git merge origin/master master\" and you are about to do this operation\nfor ALL of your repositories. Are you sure ???  [y/n]");
            wait(2);
            let mut tr = String::new();
            std::io::stdin().read_line(&mut tr);
            if tr =="y\n" {
                multi_merge(&repo_dirs, config_maker());
            }
        };
        if &temp == "4\n" {
            /*pull*/
            println!("This operation will fetch and merge your repositories..Are you sure [y/n]");
            wait(2);
            let mut tr = String::new();
            std::io::stdin().read_line(&mut tr);
            if tr =="y\n" {
                multi_pull(&repo_dirs);
            }
        };
        if &temp == "5\n" {
            /*push*/
            println!("Before pushing, please remember any uncommitted additions/deletions and modifications will not considered on github repository");
            println!("This operation will push your repositories..Are you sure [y/n]");
            wait(2);
            let mut tr = String::new();
            std::io::stdin().read_line(&mut tr);
            if tr =="y\n" {
                multi_push(&repo_dirs);
            }
        };
        if &temp == "6\n" {
            println!("Before cloning a repository be sure that destination path is empty..");
            println!("Please enter the repository url you want to clone");   /*clone dir is under repos directory*/
            let mut url = String::new();
            std::io::stdin().read_line(&mut url);
            url = url.trim().to_string();
            println!("Please enter the file path...");
            let mut path = String::new();
            std::io::stdin().read_line(&mut path);
            path=path.trim().to_string();

            clone(&url.to_string(), &path.to_string(),&config_maker());
        }
        if &temp == "7\n" {
            /*commit*/
            println!("Do you want to commit for all files ?  [y/n] ");
            let mut all_files = String::new();
            std::io::stdin().read_line(&mut all_files);
            if all_files =="y\n" {
                multi_commit(&repo_dirs);
            }
            if all_files =="n\n" {
                println!("Please enter the absolute path of the repo...");
                let mut commit_file = String::new();
                std::io::stdin().read_line(&mut commit_file);
                commit_file = commit_file.trim().to_string();
                println!("Please enter the message to add to the repo...");
                let mut commit_msg = String::new();
                std::io::stdin().read_line(&mut commit_msg);
                commit_msg = commit_msg.trim().to_string();
                let mut commit_repo = Repository::open(Path::new(&commit_file)).expect("repo opening");
                add_and_commit(&commit_repo,&commit_msg,);
            }
        };
        if &temp == "8\n" {
            /*custom command*/
            println!("Do not forget that your command executions are happening on this programs working directory.\n\
            In order to make a command execution on a specified drectory please state the preferred directory ");
            wait(2);
            println!("Please enter command...");
            let mut comd = String::new();
            std::io::stdin().read_line(&mut comd).unwrap();
            comd = comd.trim().to_string();
            let mut split = comd.split_whitespace();
            let mut com_vec : Vec<String> = Vec::new();
            for i in split{
                com_vec.push(i.to_string());
            }
            let mut pth= String::new();
            let mut cmd_nm = &(com_vec.clone()[0]);
            com_vec.remove(0);
            run_command(cmd_nm,&com_vec,&pth);
            wait(2);
        }
    }


    /*goodbye message*/


}
