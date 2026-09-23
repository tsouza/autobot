use super::*;

/// A toy reducer piece: it decides a number from a number.
trait Decide {
    fn decide(&self, input: u32) -> u32;
}

/// The port of the toy reducer.
struct DecidePort;

impl Port for DecidePort {
    type Object = dyn Decide;
    const NAME: &'static str = "Decide";
    const AWAITING: u32 = 4242;
}

/// A second toy port, to show that ports do not collide.
struct OtherPort;

impl Port for OtherPort {
    type Object = dyn Decide;
    const NAME: &'static str = "Other";
    const AWAITING: u32 = 4243;
}

struct Double;

impl Decide for Double {
    fn decide(&self, input: u32) -> u32 {
        input * 2
    }
}

struct Increment;

impl Decide for Increment {
    fn decide(&self, input: u32) -> u32 {
        input + 1
    }
}

fn double() -> Box<dyn Decide> {
    Box::new(Double)
}

fn increment() -> Box<dyn Decide> {
    Box::new(Increment)
}

/// A scenario written against the port alone, as a fixture test is.
fn scenario(resolve: impl Fn() -> Result<Box<dyn Decide>, Unresolved>) -> Result<u32, String> {
    let decide = resolve().map_err(|e| e.to_string())?;
    Ok(decide.decide(21))
}

#[test]
fn an_unregistered_port_fails_the_scenario_naming_the_port_and_its_task() {
    let result = scenario(resolve::<DecidePort>);
    assert_eq!(
        result,
        Err("no Decide implementation is registered (awaiting #4242)".to_owned())
    );
    let empty = Registry::new();
    assert_eq!(
        scenario(|| empty.resolve::<DecidePort>()),
        Err("no Decide implementation is registered (awaiting #4242)".to_owned())
    );
}

#[test]
fn a_registered_port_resolves_to_its_implementation() {
    let mut registry = Registry::new();
    registry.register::<DecidePort>(double);
    assert_eq!(scenario(|| registry.resolve::<DecidePort>()), Ok(42));
    assert_eq!(
        scenario(|| registry.resolve::<OtherPort>()),
        Err("no Other implementation is registered (awaiting #4243)".to_owned())
    );
    registry.register::<OtherPort>(increment);
    assert_eq!(scenario(|| registry.resolve::<OtherPort>()), Ok(22));
    assert_eq!(scenario(|| registry.resolve::<DecidePort>()), Ok(42));
}

#[test]
fn a_port_registered_twice_does_not_resolve() {
    let mut registry = Registry::new();
    registry.register::<DecidePort>(double);
    registry.register::<DecidePort>(increment);
    assert_eq!(registry.duplicates(), vec!["Decide"]);
    assert_eq!(
        scenario(|| registry.resolve::<DecidePort>()),
        Err("2 Decide implementations are registered; exactly one may be".to_owned())
    );
}

#[test]
fn the_installed_registry_registers_no_port_twice() {
    assert_eq!(installed().duplicates(), Vec::<&str>::new());
}
