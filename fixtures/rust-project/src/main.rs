const SCALE: i32 = 3;

fn leaf(x: i32) -> i32 { x * 2 }

fn mid(x: i32) -> i32 { leaf(x) + 1 }

fn ignores_result(x: i32) { mid(x); }

fn uses_result(x: i32) -> i32 {
    let y = mid(x);
    y
}

fn scale(x: i32) -> i32 { leaf(x) * SCALE }

struct Box { v: i32 }
impl Box {
    fn new(v: i32) -> Self { Box { v } }
    fn area(&self) -> i32 { self.v * SCALE }
}

fn make() -> Box { Box::new(mid(2)) }

fn main() { println!("{}", scale(1)); }
