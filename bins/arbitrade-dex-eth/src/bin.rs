fn main() {
    let p = ethers::types::I256::from(-10);
    println!("{:?}", p.abs().into_raw().as_u128());
}
