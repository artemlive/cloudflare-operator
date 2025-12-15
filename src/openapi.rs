use openapiv3::OpenAPI;
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, File},
    path::Path,
};

fn main() {
    println!("HERE");

    let src = "/Users/andriinasinnyk/Projects/cloudflare-operator/openapi.json";
    println!("cargo:rerun-if-changed={}", src);
    let file = File::open(src).unwrap();
    let mut spec_json: Value = serde_json::from_reader(file).unwrap();

    if let Some(paths) = spec_json.get_mut("paths").and_then(|p| p.as_object_mut()) {
        paths.iter_mut().for_each(|(path_url, methods)| {
            if let Some(methods_map) = methods.as_object_mut() {
                methods_map
                    .iter_mut()
                    .filter(|(method, _)| is_http_method(method))
                    .filter(|(_, op)| op.get("operationId").is_none())
                    .for_each(|(method, op)| {
                        op["operationId"] = json!(generate_id(method, path_url));
                    });
            }
        });
    }

    let mut generator = progenitor::Generator::default();

    let spec: OpenAPI =
        serde_json::from_value(spec_json).expect("Could not parse patched JSON into OpenAPI struct");
    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = Path::new(&env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("openapi-codegen.rs");

    fs::write(out_file, content).unwrap();
}

fn generate_id(method: &str, path: &str) -> String {
    let clean_path = path.replace(['{', '}'], "").replace(['/', '-'], "_");

    format!("{}_{}", method.to_lowercase(), clean_path)
        .trim_start_matches('_')
        .replace("__", "_")
}

fn is_http_method(m: &str) -> bool {
    matches!(
        m.to_lowercase().as_str(),
        "get" | "post" | "put" | "delete" | "patch" | "options" | "head"
    )
}
