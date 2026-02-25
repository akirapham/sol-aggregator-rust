use ethers::types::{I256, U256};
fn main() {
    let profit = I256::from(-10);
    let profit_i128 = if profit.is_negative() {
        let abs_raw = (!profit.into_raw()).overflowing_add(U256::one()).0;
        -(abs_raw.as_u128() as i128)
    } else {
        profit.into_raw().as_u128() as i128
    };
    println!("profit_i128 = {}", profit_i128);
}
