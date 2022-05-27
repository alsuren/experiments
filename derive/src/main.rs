use serde_derive::Serialize;

#[derive(Serialize)]
struct Wrapper<T> {
    thing: T,
}

fn main() {
    println!("Hello, world!");
}
