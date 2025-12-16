use controller::{account::Account, dns_record::DNSRecord, zone::Zone};
use kube::CustomResourceExt;
fn main() {
    print!("{}", serde_yaml::to_string(&DNSRecord::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&Account::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&Zone::crd()).unwrap());
}
