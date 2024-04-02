use std::{
    fmt::{Debug, Display},
    str::FromStr,
    sync::Arc,
};

use brontes_pricing::SubGraphsEntry;
use brontes_types::{
    db::{
        address_metadata::{AddressMetadata, AddressMetadataRedefined},
        address_to_protocol_info::{ProtocolInfo, ProtocolInfoRedefined},
        builder::{BuilderInfo, BuilderInfoRedefined},
        cex::{CexPriceMap, CexPriceMapRedefined},
        cex_trades::{CexTradeMap, CexTradeMapRedefined},
        clickhouse_serde::tx_trace::tx_traces_inner,
        dex::{DexKey, DexQuoteWithIndex, DexQuoteWithIndexRedefined},
        initialized_state::{InitializedStateMeta, CEX_FLAG, META_FLAG, TRACE_FLAG},
        metadata::{BlockMetadataInner, BlockMetadataInnerRedefined},
        mev_block::{MevBlockWithClassified, MevBlockWithClassifiedRedefined},
        pool_creation_block::{PoolsToAddresses, PoolsToAddressesRedefined},
        searcher::{SearcherInfo, SearcherInfoRedefined},
        token_info::TokenInfo,
        traces::{TxTracesInner, TxTracesInnerRedefined},
        traits::LibmdbxReader,
    },
    pair::Pair,
    price_graph_types::SubGraphsEntryRedefined,
    serde_utils::*,
    traits::TracingProvider,
};
use reth_db::table::Table;
use serde_with::serde_as;

use crate::{
    clickhouse::ClickhouseHandle,
    libmdbx::{types::ReturnKV, utils::protocol_info, LibmdbxData, LibmdbxReadWriter},
    parquet::ParquetExporter,
};
mod const_sql;
use alloy_primitives::Address;
use const_sql::*;
use paste::paste;
use reth_db::TableType;

use super::{
    cex_utils::CexTableFlag, initialize::LibmdbxInitializer, types::IntoTableKey, CompressedTable,
};

pub const NUM_TABLES: usize = 15;

macro_rules! tables {
    ($($table:ident),*) => {
        #[derive(Debug, PartialEq, Copy, Clone)]
        /// Default tables that should be present inside database.
        pub enum Tables {
            $(
                #[doc = concat!("Represents a ", stringify!($table), " table")]
                $table,
            )*
        }

        impl Tables {
            /// Array of all tables in database
            pub const ALL: [Tables; NUM_TABLES] = [$(Tables::$table,)*];

            /// The name of the given table in database
            pub const fn name(&self) -> &str {
                match self {
                    $(Tables::$table => {
                        $table::NAME
                    },)*
                }
            }

            /// The type of the given table in database
            pub const fn table_type(&self) -> TableType {
                match self {
                    $(Tables::$table => {
                        TableType::Table
                    },)*
                }
            }

            pub fn init_table(&self, db: &LibmdbxReadWriter) -> eyre::Result<()> {
                match self {
                    $(
                        Tables::$table => {
                            let tx = db.0.rw_tx()?;
                            tx.get_dbi::<$table>()?;
                            tx.commit()?;
                        }
                    ),*
                }

                Ok(())
            }

        }

        impl Display for Tables {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.name())
            }
        }

        impl FromStr for Tables {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($table::NAME => {
                        Ok(Tables::$table)
                    },)*
                    _ => {
                        Err("Unknown table".to_string())
                    }
                }
            }
        }
    };
}

impl Tables {
    pub(crate) async fn initialize_table<T: TracingProvider, CH: ClickhouseHandle>(
        &self,
        initializer: &LibmdbxInitializer<T, CH>,
        block_range: Option<(u64, u64)>,
        clear_table: bool,
    ) -> eyre::Result<()> {
        match self {
            Tables::TokenDecimals => {
                initializer
                    .clickhouse_init_no_args::<TokenDecimals, TokenDecimalsData>(clear_table)
                    .await
            }
            Tables::AddressToProtocolInfo => {
                initializer
                    .clickhouse_init_no_args::<AddressToProtocolInfo, AddressToProtocolInfoData>(
                        clear_table,
                    )
                    .await
            }
            Tables::PoolCreationBlocks => {
                initializer
                    .clickhouse_init_no_args::<PoolCreationBlocks, PoolCreationBlocksData>(
                        clear_table,
                    )
                    .await
            }
            Tables::CexPrice => {
                initializer
                    .initialize_table_from_clickhouse::<CexPrice, CexPriceData>(
                        block_range,
                        clear_table,
                        Some(CEX_FLAG),
                        CexTableFlag::Quotes,
                    )
                    .await
            }
            Tables::BlockInfo => {
                initializer
                    .initialize_table_from_clickhouse::<BlockInfo, BlockInfoData>(
                        block_range,
                        clear_table,
                        Some(META_FLAG),
                        CexTableFlag::default(),
                    )
                    .await
            }
            Tables::DexPrice => Ok(()),
            Tables::MevBlocks => Ok(()),
            Tables::SubGraphs => Ok(()),
            Tables::TxTraces => {
                initializer
                    .initialize_table_from_clickhouse::<TxTraces, TxTracesData>(
                        block_range,
                        clear_table,
                        Some(TRACE_FLAG),
                        CexTableFlag::default(),
                    )
                    .await
            }
            Tables::Builder => {
                initializer
                    .clickhouse_init_no_args::<Builder, BuilderData>(clear_table)
                    .await
            }
            Tables::AddressMeta => {
                initializer
                    .clickhouse_init_no_args::<AddressMeta, AddressMetaData>(clear_table)
                    .await
            }
            Tables::SearcherEOAs => Ok(()),
            Tables::SearcherContracts => Ok(()),
            Tables::InitializedState => Ok(()),
            Tables::CexTrades => {
                initializer
                    .initialize_table_from_clickhouse::<CexTrades, CexTradesData>(
                        block_range,
                        clear_table,
                        Some(CEX_FLAG),
                        CexTableFlag::Trades,
                    )
                    .await
            }
        }
    }

    pub(crate) async fn initialize_table_arbitrary_state<
        T: TracingProvider,
        CH: ClickhouseHandle,
    >(
        &self,
        initializer: &LibmdbxInitializer<T, CH>,
        block_range: &'static [u64],
    ) -> eyre::Result<()> {
        match self {
            Tables::TokenDecimals => {
                unimplemented!(
                    "'initialize_table_arbitrary_state' not implemented for token decimals"
                );
            }
            Tables::AddressToProtocolInfo => {
                unimplemented!(
                    "'initialize_table_arbitrary_state' not implemented for AddressToProtocolInfo"
                );
            }
            Tables::PoolCreationBlocks => {
                unimplemented!(
                    "'initialize_table_arbitrary_state' not implemented for PoolCreationBlocks"
                );
            }
            Tables::CexPrice => {
                initializer
                    .initialize_table_from_clickhouse_arbitrary_state::<CexPrice, CexPriceData>(
                        block_range,
                        Some(CEX_FLAG),
                        CexTableFlag::Quotes,
                    )
                    .await
            }
            Tables::BlockInfo => {
                initializer
                    .initialize_table_from_clickhouse_arbitrary_state::<BlockInfo, BlockInfoData>(
                        block_range,
                        Some(META_FLAG),
                        CexTableFlag::default(),
                    )
                    .await
            }
            Tables::DexPrice => Ok(()),
            Tables::MevBlocks => Ok(()),
            Tables::SubGraphs => Ok(()),
            Tables::TxTraces => {
                initializer
                    .initialize_table_from_clickhouse_arbitrary_state::<TxTraces, TxTracesData>(
                        block_range,
                        Some(TRACE_FLAG),
                        CexTableFlag::default(),
                    )
                    .await
            }
            Tables::Builder => {
                unimplemented!("'initialize_table_arbitrary_state' not implemented for Builder");
            }
            Tables::AddressMeta => {
                unimplemented!(
                    "'initialize_table_arbitrary_state' not implemented for AddressMeta"
                );
            }
            Tables::SearcherEOAs => Ok(()),
            Tables::SearcherContracts => Ok(()),
            Tables::InitializedState => Ok(()),
            Tables::CexTrades => {
                initializer
                    .initialize_table_from_clickhouse_arbitrary_state::<CexTrades, CexTradesData>(
                        block_range,
                        Some(CEX_FLAG),
                        CexTableFlag::Trades,
                    )
                    .await
            }
        }
    }

    pub async fn export_to_parquet<DB>(
        &self,
        exporter: Arc<ParquetExporter<DB>>,
    ) -> eyre::Result<()>
    where
        DB: LibmdbxReader,
    {
        match self {
            Self::AddressMeta => exporter.export_address_metadata().await,
            Self::MevBlocks => exporter.export_mev_blocks().await,
            Self::SearcherContracts | Self::SearcherEOAs => exporter.export_searcher_info().await,
            Self::Builder => exporter.export_builder_info().await,
            _ => unreachable!("Parquet export not yet supported for this table"),
        }
    }
}

tables!(
    TokenDecimals,
    AddressToProtocolInfo,
    CexPrice,
    BlockInfo,
    DexPrice,
    PoolCreationBlocks,
    MevBlocks,
    SubGraphs,
    TxTraces,
    Builder,
    AddressMeta,
    SearcherEOAs,
    SearcherContracts,
    InitializedState,
    CexTrades
);

/// Must be in this order when defining
/// Table {
///     Data {
///     },
///     Init {
///     }
///     CLI
///     }
/// }
macro_rules! compressed_table {
    (Table $(#[$attrs:meta])* $table_name:ident { $($head:tt)* }) => {
        compressed_table!($(#[$attrs:meta])* $table_name {} $($head)*);
    };
    (
        $(#[$attrs:meta])* $table_name:ident,
        $c_val:ident, $decompressed_value:ident, $key:ident
        { $($acc:tt)* } $(,)*
    ) => {
        $(#[$attrs])*
        #[doc = concat!("Takes [`", stringify!($key), "`] as a key and returns [`", stringify!($value), "`].")]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $table_name;

        impl reth_db::table::Table for $table_name {
            // this type is needed for the trait impl but we never actually use it,
            // so an arbitrary table will do
            const TABLE: reth_db::Tables = reth_db::Tables::CanonicalHeaders;
            const NAME: &'static str = stringify!($table_name);
            type Key = $key;
            type Value = $c_val;
        }

        impl std::fmt::Display for $table_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", stringify!($table_name))
            }
        }

        #[cfg(feature = "tests")]
        #[allow(unused)]
        impl $table_name {
            pub async fn test_initialized_data<CH: ClickhouseHandle>(
                clickhouse: &CH,
                libmdbx: &crate::libmdbx::LibmdbxReadWriter,
                block_range: Option<(u64, u64)>
            ) -> eyre::Result<()> {
                paste::paste!{
                    crate::libmdbx::test_utils::compare_clickhouse_libmdbx_data
                    ::<$table_name,[<$table_name Data>], CH>(clickhouse, libmdbx, block_range).await
                }
            }

            pub async fn test_initialized_arbitrary_data<CH: ClickhouseHandle>(
                clickhouse: &CH,
                libmdbx: &crate::libmdbx::LibmdbxReadWriter,
                block_range: &'static [u64]
            ) -> eyre::Result<()> {
                paste::paste!{
                    crate::libmdbx::test_utils::compare_clickhouse_libmdbx_arbitrary_data
                    ::<$table_name,[<$table_name Data>], CH>(clickhouse, libmdbx, block_range).await
                }
            }
        }

        $($acc)*
    };
    (
        $(#[$attrs:meta])* $table_name:ident
        { $($acc:tt)* } $(#[$dattrs:meta])*
        Data {
            $(#[$kattrs:meta])* key: $key:ident,
            $(#[$vattrs:meta])* value: $val:ident,
            $(#[$vcattrs:meta])* compressed_value: $c_val:ident
        },  $($tail:tt)*
    ) => {
        compressed_table!($(#[$attrs])* $table_name { $($acc)* }
                          Data  {
                              $(#[$kattrs])* key: $key,
                              $(#[$vattrs])* value: $val,
                              $(#[$vcattrs])* compressed_value: $c_val false
                          }, $($tail)*);

    };
    // parse key value compressed
    ($(#[$attrs:meta])* $table_name:ident
     { $($acc:tt)* } $(#[$dattrs:meta])*
        Data {
            $(#[$kattrs:meta])* key: $key:ident,
            $(#[$vattrs:meta])* value: $val:ident,
            $(#[$vcattrs:meta])* compressed_value: $c_val:ident false
        },  $($tail:tt)*) => {
        compressed_table!($(#[$attrs])* $table_name, $c_val, $val, $key {
        $($acc)*
        paste!(
        #[derive(Debug, Clone, Default, clickhouse::Row, serde::Serialize, serde::Deserialize)]
        $(#[$dattrs])*
        pub struct [<$table_name Data>] {
            $(#[$kattrs])*
            pub key: $key,
            $(#[$vattrs])*
            pub value: $val
        }

        impl [<$table_name Data>] {
            pub fn new(key: $key, value: $val) -> Self {
                [<$table_name Data>] {
                    key,
                    value
                }
            }
        }

        impl From<($key, $val)> for [<$table_name Data>] {
            fn from(value: ($key, $val)) -> Self {
                [<$table_name Data>] {
                    key: value.0,
                    value: value.1
                }
            }
        }

        impl LibmdbxData<$table_name> for [<$table_name Data>] {
            fn into_key_val(&self) -> ReturnKV<$table_name> {
                (self.key.clone(), self.value.clone()).into()
            }
        }

        );
    } $($tail)*);
    };
    ($(#[$attrs:meta])* $table_name:ident { $($acc:tt)* } $(#[$dattrs:meta])*
     Data {
         $(#[$kattrs:meta])* key: $key:ident,
         $(#[$vattrs:meta])* value: $val:ident
     },  $($tail:tt)*) => {
        compressed_table!($(#[$attrs])* $table_name { $($acc)* } $(#[$dattrs])*
                          Data  {
                              $(#[$kattrs])* key: $key,
                              $(#[$vattrs])* value: $val,
                              $(#[$vattrs])* compressed_value: $val false
                          }, $($tail)*);

    };
    ($(#[$attrs:meta])* $table_name:ident, $c_val:ident, $decompressed_value:ident, $key:ident
     { $($acc:tt)* } Init { init_size: $init_chunk_size:expr, init_method: Clickhouse,
                              http_endpoint: $http_endpoint:expr },

     $($tail:tt)*) => {
        compressed_table!($(#[$attrs])* $table_name, $c_val, $decompressed_value, $key {
            $($acc)*
        impl CompressedTable for $table_name {
            type DecompressedValue = $decompressed_value;
            const INIT_CHUNK_SIZE: Option<usize> = $init_chunk_size;
            const INIT_QUERY: Option<&'static str> = Some(paste! {[<$table_name InitQuery>]});
            const HTTP_ENDPOINT: Option<&'static str> = $http_endpoint;
        }
        } $($tail)*);
    };
    ($(#[$attrs:meta])* $table_name:ident, $c_val:ident, $decompressed_value:ident, $key:ident
     { $($acc:tt)* } Init { init_size: $init_chunk_size:expr, init_method: Other,
     http_endpoint: $http_endpoint:expr  },
     $($tail:tt)*) => {
        compressed_table!($(#[$attrs])* $table_name, $c_val, $decompressed_value, $key {
            $($acc)*
        impl CompressedTable for $table_name {
            type DecompressedValue = $decompressed_value;
            const INIT_CHUNK_SIZE: Option<usize> = $init_chunk_size;
            const INIT_QUERY: Option<&'static str> = None;
            const HTTP_ENDPOINT: Option<&'static str> = $http_endpoint;
        }
        } $($tail)*);
    };
    ($(#[$attrs:meta])* $table_name:ident, $c_val:ident, $decompressed_value:ident, $key:ident
     { $($acc:tt)* } CLI { can_insert: False }  $($tail:tt)*) => {
        compressed_table!($(#[$attrs])* $table_name, $c_val, $decompressed_value, $key {
            $($acc)*
        impl IntoTableKey<&str, $key, paste!([<$table_name Data>])> for $table_name {
            fn into_key(value: &str) -> $key {
                let key: $key = value.parse().unwrap();
                println!("decoded key: {key:?}");
                key
            }
            fn into_table_data(_: &str, _: &str) -> paste!([<$table_name Data>]) {
                panic!("inserts not supported for $table_name");
            }
        }
        } $($tail)*);
    };
    ($(#[$attrs:meta])* $table_name:ident,$c_val:ident, $decompressed_value:ident, $key:ident
     { $($acc:tt)* } CLI { can_insert: True }  $($tail:tt)*) => {

        compressed_table!($(#[$attrs])* $table_name, $c_val, $decompressed_value, $key {
            $($acc)*
        impl IntoTableKey<&str, $key, paste!([<$table_name Data>])> for $table_name {
            fn into_key(value: &str) -> $key {
                let key: $key = value.parse().unwrap();
                println!("decoded key: {key:?}");
                key
            }
            fn into_table_data(key: &str, value: &str) -> paste!([<$table_name Data>]) {
                let key: $key = key.parse().unwrap();
                let value: $decompressed_value = value.parse().unwrap();
                <paste!([<$table_name Data>])>::new(key, value)
            }
        }
        } $($tail)*);
    };

}

compressed_table!(
    Table TokenDecimals {
        #[serde_as]
        Data {
            #[serde(with = "address_string")]
            key: Address,
            value: TokenInfo
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: Some("token-decimals")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table AddressToProtocolInfo {
        #[serde_as]
        Data {
            #[serde(with = "address_string")]
            key: Address,
            #[serde(with = "protocol_info")]
            value: ProtocolInfo,
            compressed_value: ProtocolInfoRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: Some("protocol-info")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table CexPrice {
        Data {
        key: u64,
        value: CexPriceMap,
        compressed_value: CexPriceMapRedefined
        },
        Init {
            init_size: Some(3_500),
            init_method: Clickhouse,
            http_endpoint: Some("cex-price")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table BlockInfo {
        #[serde_as]
        Data {
            key: u64,
            value: BlockMetadataInner,
            compressed_value: BlockMetadataInnerRedefined
        },
        Init {
            init_size: Some(500_000),
            init_method: Clickhouse,
            http_endpoint: Some("block-info")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table DexPrice {
        Data {
            key: DexKey,
            value: DexQuoteWithIndex,
            compressed_value: DexQuoteWithIndexRedefined
        },
        Init {
            init_size: None,
            init_method: Other,
            http_endpoint: Some("dex-pricing")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table PoolCreationBlocks {
        #[serde_as]
        Data {
            key: u64,
            #[serde(with = "pools_libmdbx")]
            value: PoolsToAddresses,
            compressed_value: PoolsToAddressesRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: Some("pool-creation-blocks")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table MevBlocks {
        Data {
            key: u64,
            value: MevBlockWithClassified,
            compressed_value: MevBlockWithClassifiedRedefined
        },
        Init {
            init_size: None,
            init_method: Other,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table SubGraphs {
        Data {
            key: Pair,
            value: SubGraphsEntry,
            compressed_value: SubGraphsEntryRedefined
        },
        Init {
            init_size: None,
            init_method: Other,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table TxTraces {
        #[serde_as]
        Data {
            key: u64,
            #[serde(deserialize_with = "tx_traces_inner::deserialize")]
            value: TxTracesInner,
            compressed_value: TxTracesInnerRedefined
        },
        Init {
            init_size: Some(50_000),
            init_method: Clickhouse,
            http_endpoint: Some("tx-traces")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table Builder {
        #[serde_as]
        Data {
            #[serde(with = "address_string")]
            key: Address,
            value: BuilderInfo,
            compressed_value: BuilderInfoRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: Some("builder")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table AddressMeta {
        Data {
            #[serde(with = "address_string")]
            key: Address,
            value: AddressMetadata,
            compressed_value: AddressMetadataRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: Some("address-meta")
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table SearcherEOAs {
        Data {
            #[serde(with = "address_string")]
            key: Address,
            value: SearcherInfo,
            compressed_value: SearcherInfoRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table SearcherContracts {
        Data {
            #[serde(with = "address_string")]
            key: Address,
            value: SearcherInfo,
            compressed_value: SearcherInfoRedefined
        },
        Init {
            init_size: None,
            init_method: Clickhouse,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table InitializedState {
        Data {
            key: u64,
            value: InitializedStateMeta,
            compressed_value: InitializedStateMeta
        },
        Init {
            init_size: None,
            init_method: Other,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);

compressed_table!(
    Table CexTrades {
        Data {
        key: u64,
        value: CexTradeMap,
        compressed_value: CexTradeMapRedefined
        },
        Init {
            init_size: Some(3_500),
            init_method: Clickhouse,
            http_endpoint: None
        },
        CLI {
            can_insert: False
        }
    }
);
