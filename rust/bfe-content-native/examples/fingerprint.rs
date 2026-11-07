//! Inspect the same gameplay agreement used by a native online connection.
use bfe_content_native::fingerprint;
use bfe_content::files::DirFiles;
fn main(){
    let mut args=std::env::args().skip(1);let root=args.next().expect("content root required");let query=args.next().unwrap_or_default();let files=DirFiles::new(root);
    match (fingerprint::hash(&files,&query),fingerprint::manifest(&files,&query)){
        (Ok(hash),Ok(manifest))=>println!("{}",serde_json::json!({"hash":hash,"manifest":manifest})),
        (Err(error),_)|(_,Err(error))=>{eprintln!("{error}");std::process::exit(1);}
    }
}
