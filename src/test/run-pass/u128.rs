#![feature(i128_type)]

fn main() {
    let x: u128 = 0xFFFF_FFFF_FFFF_FFFF__FFFF_FFFF_FFFF_FFFF;
    assert_eq!(0, !x);
    assert_eq!(0, !x);
    let y: u128 = 0xFFFF_FFFF_FFFF_FFFF__FFFF_FFFF_FFFF_FFFE;
    assert_eq!(!1, y);
    assert_eq!(x, y | 1);
    assert_eq!(0xFAFF_0000_FF8F_0000__FFFF_0000_FFFF_FFFE,
               y &
               0xFAFF_0000_FF8F_0000__FFFF_0000_FFFF_FFFF);
    let z: u128 = 0xABCD_EF;
    assert_eq!(z * z * z * z, 0x33EE_0E2A_54E2_59DA_A0E7_8E41);
    assert_eq!(z + z + z + z, 0x2AF3_7BC);
    let k: u128 = 0x1234_5678_9ABC_DEFF_EDCB_A987_6543_210;
    assert_eq!(k + k, 0x2468_ACF1_3579_BDFF_DB97_530E_CA86_420);
    assert_eq!(0, k - k);
    assert_eq!(0x1234_5678_9ABC_DEFF_EDCB_A987_5A86_421, k - z);
    assert_eq!(0x1000_0000_0000_0000_0000_0000_0000_000,
               k - 0x234_5678_9ABC_DEFF_EDCB_A987_6543_210);
    assert_eq!(0x6EF5_DE4C_D3BC_2AAA_3BB4_CC5D_D6EE_8, k / 42);
    assert_eq!(0, k % 42);
    assert_eq!(15, z % 42);
    assert_eq!(0x91A2_B3C4_D5E6_F7, k >> 65);
    assert_eq!(0xFDB9_7530_ECA8_6420_0000_0000_0000_0000, k << 65);
    assert!(k > z);
    assert!(y > k);
    assert!(y < x);
    assert_eq!(x as u64, !0);
    assert_eq!(z as u64, 0xABCD_EF);
    assert_eq!(k as u64, 0xFEDC_BA98_7654_3210);
    assert_eq!(k as i128, 0x1234_5678_9ABC_DEFF_EDCB_A987_6543_210);
    // formatting
    let j: u128 = 1 << 67;
    assert_eq!("147573952589676412928", format!("{}", j));
    assert_eq!("80000000000000000", format!("{:x}", j));
    assert_eq!("20000000000000000000000", format!("{:o}", j));
    assert_eq!("10000000000000000000000000000000000000000000000000000000000000000000",
               format!("{:b}", j));
    assert_eq!("147573952589676412928", format!("{:?}", j));
    // common traits
    x.clone();
}
