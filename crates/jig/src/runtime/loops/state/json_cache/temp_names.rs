fn temporary_file_prefix(data_name: &OsStr) -> OsString {
    let mut prefix = data_name.to_os_string();
    prefix.push(".tmp-");
    prefix
}

fn temporary_file_name(data_name: &OsStr) -> OsString {
    let mut name = temporary_file_prefix(data_name);
    name.push(Ulid::new().to_string());
    name
}
