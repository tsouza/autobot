use autobot_kernel::fields::FieldClasses;

#[derive(PartialEq, FieldClasses)]
struct Status {
    #[field(canonical)]
    unknown: u32,
    #[field(domain, control)]
    two_classes: u32,
    #[field(domain)]
    #[field(domain)]
    repeated: u32,
    #[field()]
    empty: u32,
}

fn main() {}
