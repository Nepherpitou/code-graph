fn a(n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        b(n - 1)
    }
}

fn b(n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        a(n - 1)
    }
}

fn unused_helper() -> i32 {
    41
}

fn main() {
    println!("{}", a(3));
}
