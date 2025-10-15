use std::io::BufRead;

pub trait DynamicRead: BufRead {
    fn grow(&mut self);
    fn shrink(&mut self);
    fn discard(&mut self);
    fn compact(&mut self);
}
