//! Provides a set of utilities and helpers for testing inspectors within the
//! `brontes-inspect` crate. This includes functions for creating transaction
//! trees, applying pricing information, and running inspectors with various
//! configurations to assert expected MEV (Miner Extractable Value) outcomes.
//!
//! ## Key Components
//!
//! - `InspectorTestUtils`: A struct providing methods to facilitate the testing
//!   of inspectors.
//! - `InspectorTxRunConfig`: Configuration struct for running single
//!   opportunity tests with inspectors.
//! - `ComposerRunConfig`: Configuration struct for running composition tests
//!   across multiple inspectors.
//! - `InspectorTestUtilsError`: Enum defining possible error types that can
//!   occur during test execution.
//!
//! ## Usage
//!
//! Test utilities are primarily used in the context of unit and integration
//! tests to verify the correctness of inspector implementations. They allow for
//! detailed configuration of test scenarios, including specifying transaction
//! hashes, blocks, expected profits, and gas usage, among other parameters.

use alloy_primitives::{Address, TxHash};
use brontes_classifier::test_utils::{ClassifierTestUtils, ClassifierTestUtilsError};
use brontes_core::TraceLoaderError;
pub use brontes_types::constants::*;
use brontes_types::{
    db::{
        cex::CexExchange,
        dex::DexQuotes,
        metadata::{MetadataCombined, MetadataNoDex},
    },
    mev::{Bundle, MevType},
    normalized_actions::Actions,
    tree::BlockTree,
};
use thiserror::Error;

use crate::{composer::compose_mev_results, Inspectors};

/// Inspector Specific testing functionality
pub struct InspectorTestUtils {
    classifier_inspector:  ClassifierTestUtils,
    quote_address:         Address,
    max_result_difference: f64,
}

impl InspectorTestUtils {
    pub fn new(quote_address: Address, max_result_difference: f64) -> Self {
        let classifier_inspector = ClassifierTestUtils::new();
        Self { classifier_inspector, quote_address, max_result_difference }
    }

    async fn get_tree_txes(
        &self,
        tx_hashes: Vec<TxHash>,
    ) -> Result<BlockTree<Actions>, InspectorTestUtilsError> {
        let mut trees = self.classifier_inspector.build_tree_txes(tx_hashes).await?;

        if trees.len() != 1 {
            return Err(InspectorTestUtilsError::MultipleBlockError(
                trees.into_iter().map(|t| t.header.number).collect(),
            ))
        }
        Ok(trees.remove(0))
    }

    async fn get_tree_txes_with_pricing(
        &self,
        tx_hashes: Vec<TxHash>,
    ) -> Result<(BlockTree<Actions>, DexQuotes), InspectorTestUtilsError> {
        let mut trees = self
            .classifier_inspector
            .build_tree_txes_with_pricing(tx_hashes, self.quote_address)
            .await?;

        if trees.len() != 1 {
            return Err(InspectorTestUtilsError::MultipleBlockError(
                trees.into_iter().map(|(t, _)| t.header.number).collect(),
            ))
        }
        Ok(trees.remove(0))
    }

    fn default_metadata(&self) -> MetadataCombined {
        MetadataCombined { db: MetadataNoDex::default(), dex_quotes: DexQuotes(vec![]) }
    }

    async fn get_block_tree(
        &self,
        block: u64,
    ) -> Result<BlockTree<Actions>, InspectorTestUtilsError> {
        self.classifier_inspector
            .build_block_tree(block)
            .await
            .map_err(Into::into)
    }

    async fn get_block_tree_with_pricing(
        &self,
        block: u64,
    ) -> Result<(BlockTree<Actions>, DexQuotes), InspectorTestUtilsError> {
        self.classifier_inspector
            .build_block_tree_with_pricing(block, self.quote_address)
            .await
            .map_err(Into::into)
    }

    pub async fn assert_no_mev(
        &self,
        config: InspectorTxRunConfig,
    ) -> Result<(), InspectorTestUtilsError> {
        let copied = config.clone();
        let err = || InspectorTestUtilsError::InspectorConfig(copied.clone());

        let mut quotes = None;
        let tree = if let Some(tx_hashes) = config.mev_tx_hashes {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_tree_txes_with_pricing(tx_hashes).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_tree_txes(tx_hashes).await?
            }
        } else if let Some(block) = config.block {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_block_tree_with_pricing(block).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_block_tree(block).await?
            }
        } else {
            return Err(err())
        };

        let block = tree.header.number;

        let mut metadata = if let Some(meta) = config.metadata_override {
            meta
        } else {
            self.classifier_inspector.get_metadata(block).await?
        };

        if let Some(quotes) = quotes {
            metadata.dex_quotes = quotes;
        }

        if metadata.dex_quotes.0.is_empty() && config.needs_dex_prices {
            assert!(false, "no dex quotes found in metadata. test suite will fail");
        }

        let inspector = config.expected_mev_type.init_inspector(
            self.quote_address,
            self.classifier_inspector.libmdbx,
            &vec![CexExchange::Binance],
        );

        let results = inspector.process_tree(tree.into(), metadata.into()).await;
        assert_eq!(results.len(), 0, "found mev when we shouldn't of {:#?}", results);

        Ok(())
    }

    pub async fn run_inspector(
        &self,
        config: InspectorTxRunConfig,
        specific_state_tests: Option<Box<dyn for<'a> Fn(&'a Bundle)>>,
    ) -> Result<(), InspectorTestUtilsError> {
        let copied = config.clone();
        let err = || InspectorTestUtilsError::InspectorConfig(copied.clone());

        let profit_usd = config.expected_profit_usd.ok_or_else(err)?;
        let gas_used_usd = config.expected_gas_usd.ok_or_else(err)?;

        let mut quotes = None;
        let tree = if let Some(tx_hashes) = config.mev_tx_hashes {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_tree_txes_with_pricing(tx_hashes).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_tree_txes(tx_hashes).await?
            }
        } else if let Some(block) = config.block {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_block_tree_with_pricing(block).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_block_tree(block).await?
            }
        } else {
            return Err(err())
        };

        let block = tree.header.number;

        let mut metadata = if let Some(meta) = config.metadata_override {
            meta
        } else {
            let res = self.classifier_inspector.get_metadata(block).await;
            if config.expected_mev_type == Inspectors::CexDex {
                res?
            } else {
                res.unwrap_or_else(|_| self.default_metadata())
            }
        };

        if let Some(quotes) = quotes {
            metadata.dex_quotes = quotes;
        }

        if metadata.dex_quotes.0.is_empty() && config.needs_dex_prices {
            assert!(false, "no dex quotes found in metadata. test suite will fail");
        }

        let inspector = config.expected_mev_type.init_inspector(
            self.quote_address,
            self.classifier_inspector.libmdbx,
            &vec![
                CexExchange::Binance,
                CexExchange::Coinbase,
                CexExchange::Okex,
                CexExchange::BybitSpot,
                CexExchange::Kucoin,
            ],
        );

        let mut results = inspector.process_tree(tree.into(), metadata.into()).await;

        assert_eq!(
            results.len(),
            1,
            "Identified an incorrect number of MEV bundles. Expected 1, found: {}",
            results.len()
        );

        let bundle = results.remove(0);

        if let Some(specific_state_tests) = specific_state_tests {
            specific_state_tests(&bundle);
        }

        // check gas
        assert!(
            (bundle.header.bribe_usd - gas_used_usd).abs() < self.max_result_difference,
            "Finalized Bribe != Expected Bribe, {} != {}",
            bundle.header.bribe_usd,
            gas_used_usd
        );
        // check profit
        assert!(
            (bundle.header.profit_usd - profit_usd).abs() < self.max_result_difference,
            "Finalized Profit != Expected Profit, {} != {}",
            bundle.header.profit_usd,
            profit_usd
        );

        Ok(())
    }

    pub async fn run_composer(
        &self,
        config: ComposerRunConfig,
        specific_state_tests: Option<Box<dyn for<'a> Fn(&'a Bundle)>>,
    ) -> Result<(), InspectorTestUtilsError> {
        let copied = config.clone();
        let err = || InspectorTestUtilsError::ComposerConfig(copied.clone());

        let profit_usd = config.expected_profit_usd.ok_or_else(err)?;
        let gas_used_usd = config.expected_gas_usd.ok_or_else(err)?;

        let mut quotes = None;
        let tree = if let Some(tx_hashes) = config.mev_tx_hashes {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_tree_txes_with_pricing(tx_hashes).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_tree_txes(tx_hashes).await?
            }
        } else if let Some(block) = config.block {
            if config.needs_dex_prices {
                let (tree, prices) = self.get_block_tree_with_pricing(block).await?;
                quotes = Some(prices);
                tree
            } else {
                self.get_block_tree(block).await?
            }
        } else {
            return Err(err())
        };

        let block = tree.header.number;

        let mut metadata = if let Some(meta) = config.metadata_override {
            meta
        } else {
            let res = self.classifier_inspector.get_metadata(block).await;
            if config.inspectors.contains(&Inspectors::CexDex) {
                res?
            } else {
                res.unwrap_or_else(|_| self.default_metadata())
            }
        };

        if let Some(quotes) = quotes {
            metadata.dex_quotes = quotes;
        }

        if metadata.dex_quotes.0.is_empty() && config.needs_dex_prices {
            assert!(false, "no dex quotes found in metadata. test suite will fail");
        }

        let inspector = config
            .inspectors
            .into_iter()
            .map(|i| {
                i.init_inspector(
                    self.quote_address,
                    self.classifier_inspector.libmdbx,
                    &vec![CexExchange::Binance],
                )
            })
            .collect::<Vec<_>>();

        let results = compose_mev_results(inspector.as_slice(), tree.into(), metadata.into()).await;

        let mut results = results
            .mev_details
            .into_iter()
            .filter(|mev| {
                config
                    .prune_opportunities
                    .as_ref()
                    .map(|opp| !opp.contains(&mev.header.tx_hash))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            results.len(),
            1,
            "Got wrong number of mev bundles. Expected 1, got {}",
            results.len()
        );

        let bundle = results.remove(0);
        assert!(
            bundle.header.mev_type == config.expected_mev_type,
            "got wrong composed type {} != {}",
            bundle.header.mev_type,
            config.expected_mev_type
        );

        if let Some(specific_state_tests) = specific_state_tests {
            specific_state_tests(&bundle);
        }

        // check gas
        assert!(
            (bundle.header.bribe_usd - gas_used_usd).abs() < self.max_result_difference,
            "Finalized Bribe != Expected Bribe, {} != {}",
            bundle.header.bribe_usd,
            gas_used_usd
        );
        // check profit
        assert!(
            (bundle.header.profit_usd - profit_usd).abs() < self.max_result_difference,
            "Finalized Profit != Expected Profit, {} != {}",
            bundle.header.profit_usd,
            profit_usd
        );

        Ok(())
    }
}

/// This inspector test config is to configure an inspector test for a single
/// bundle. MevTxHashes is a list of tx hashes that are expected be in the
/// bundle.
#[derive(Debug, Clone)]
pub struct InspectorTxRunConfig {
    pub metadata_override:   Option<MetadataCombined>,
    pub mev_tx_hashes:       Option<Vec<TxHash>>,
    pub block:               Option<u64>,
    pub expected_profit_usd: Option<f64>,
    pub expected_gas_usd:    Option<f64>,
    pub expected_mev_type:   Inspectors,
    pub needs_dex_prices:    bool,
}

impl InspectorTxRunConfig {
    pub fn new(mev: Inspectors) -> Self {
        Self {
            expected_mev_type:   mev,
            block:               None,
            mev_tx_hashes:       None,
            expected_profit_usd: None,
            expected_gas_usd:    None,
            metadata_override:   None,
            needs_dex_prices:    false,
        }
    }

    pub fn with_dex_prices(mut self) -> Self {
        self.needs_dex_prices = true;
        self
    }

    pub fn with_block(mut self, block: u64) -> Self {
        self.block = Some(block);
        self
    }

    pub fn with_metadata_override(mut self, metadata: MetadataCombined) -> Self {
        self.metadata_override = Some(metadata);
        self
    }

    pub fn with_mev_tx_hashes(mut self, txes: Vec<TxHash>) -> Self {
        self.mev_tx_hashes = Some(txes);
        self
    }

    pub fn with_expected_profit_usd(mut self, profit: f64) -> Self {
        self.expected_profit_usd = Some(profit);
        self
    }

    /// Total cost of transaction in USD. This includes base fee, priority fee &
    /// bribe
    pub fn with_gas_paid_usd(mut self, gas: f64) -> Self {
        self.expected_gas_usd = Some(gas);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ComposerRunConfig {
    pub inspectors:          Vec<Inspectors>,
    pub expected_mev_type:   MevType,
    pub metadata_override:   Option<MetadataCombined>,
    pub mev_tx_hashes:       Option<Vec<TxHash>>,
    pub block:               Option<u64>,
    pub expected_profit_usd: Option<f64>,
    pub expected_gas_usd:    Option<f64>,
    pub prune_opportunities: Option<Vec<TxHash>>,
    pub needs_dex_prices:    bool,
}

impl ComposerRunConfig {
    pub fn new(inspectors: Vec<Inspectors>, expected_mev_type: MevType) -> Self {
        Self {
            inspectors,
            metadata_override: None,
            mev_tx_hashes: None,
            expected_mev_type,
            block: None,
            expected_profit_usd: None,
            expected_gas_usd: None,
            prune_opportunities: None,
            needs_dex_prices: false,
        }
    }

    pub fn with_metadata_override(mut self, metadata: MetadataCombined) -> Self {
        self.metadata_override = Some(metadata);
        self
    }

    pub fn with_mev_tx_hashes(mut self, txes: Vec<TxHash>) -> Self {
        self.mev_tx_hashes = Some(txes);
        self
    }

    pub fn with_block(mut self, block: u64) -> Self {
        self.block = Some(block);
        self
    }

    pub fn with_expected_profit_usd(mut self, profit: f64) -> Self {
        self.expected_profit_usd = Some(profit);
        self
    }

    pub fn with_gas_paid_usd(mut self, gas: f64) -> Self {
        self.expected_gas_usd = Some(gas);
        self
    }

    pub fn with_prune_opportunities(mut self, prune_txes: Vec<TxHash>) -> Self {
        self.prune_opportunities = Some(prune_txes);
        self
    }

    pub fn with_dex_prices(mut self) -> Self {
        self.needs_dex_prices = true;
        self
    }
}

#[derive(Debug, Error)]
pub enum InspectorTestUtilsError {
    #[error(transparent)]
    Classification(#[from] ClassifierTestUtilsError),
    #[error(transparent)]
    Tracing(#[from] TraceLoaderError),
    #[error("invalid inspector tx run config: {0:?}")]
    InspectorConfig(InspectorTxRunConfig),
    #[error("invalid composer run config: {0:?}")]
    ComposerConfig(ComposerRunConfig),
    #[error("no inspector for type: {0}")]
    MissingInspector(MevType),
    #[error("more than one block found in inspector config. blocks: {0:?}")]
    MultipleBlockError(Vec<u64>),
}
