use ethers::types::I256;
#[test]
fn test_i256_conv() {
    let p = I256::from(-10);
    // let temp = (!p.into_raw()).overflowing_add(ethers::types::U256::one()).0;
    // let val = -(temp.as_u128() as i128);
    println!("WORKS: {:?}", p);
}
