use controller::{dns_record::DNSRecord, zone::Zone};
use kube::CustomResourceExt;
fn main() {
    print!("{}", serde_yaml::to_string(&DNSRecord::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&Zone::crd()).unwrap());
}
