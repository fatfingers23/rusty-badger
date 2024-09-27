use core::fmt::Arguments;
use heapless::String;

/// Makes it easier to format strings in a single line method
pub fn easy_format<const N: usize>(args: Arguments<'_>) -> String<N> {
    let mut formatted_string: String<N> = String::<N>::new();
    let result = core::fmt::write(&mut formatted_string, args);
    match result {
        Ok(_) => formatted_string,
        Err(_) => {
            panic!("Error formatting the string")
        }
    }
}
