use std::any::Any;

pub trait Reflect: Any {
    fn type_name(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn field(&self, name: &str) -> Option<&dyn Reflect>;
    fn field_mut(&mut self, name: &str) -> Option<&mut dyn Reflect>;
    fn fields(&self) -> Vec<(&'static str, &dyn Reflect)>;
}