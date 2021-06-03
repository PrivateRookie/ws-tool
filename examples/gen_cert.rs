use rcgen;
use structopt::StructOpt;

/// to generate ssl cert and key files used by wss
#[derive(StructOpt)]
struct Args {
    /// domain name
    domain: Vec<String>,
}

fn main() {
    let args = Args::from_args();
    let cert = rcgen::generate_simple_self_signed(args.domain).expect("unable to generate certs");
    println!("{}", cert.serialize_pem().unwrap());
    println!("{}", cert.serialize_private_key_pem());
}
