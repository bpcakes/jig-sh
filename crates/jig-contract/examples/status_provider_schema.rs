use jig_contract::status_provider::v1;

fn main() {
    let schema = v1::schema();
    let json = serde_json::to_string_pretty(&schema)
        .expect("the generated status-provider schema must serialize");
    println!("{json}");
}
