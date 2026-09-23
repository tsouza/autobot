use autobot_kernel::fields::FieldClasses;

#[derive(PartialEq, FieldClasses)]
struct Status {
    #[field(domain)]
    state: u32,
    hold_state: u32,
}

fn main() {}
