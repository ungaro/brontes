use alloy_primitives::{hex, Address};

pub const WETH_ADDRESS: Address = Address::new(hex!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"));
pub const USDT_ADDRESS: Address = Address::new(hex!("dAC17F958D2ee523a2206206994597C13D831ec7"));
pub const USDC_ADDRESS: Address = Address::new(hex!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"));

/// The first block where the chainbound mempool data is available.
pub const START_OF_CHAINBOUND_MEMPOOL_DATA: u64 = 17193367;
