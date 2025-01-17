#![cfg(test)]
use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use rust_decimal_macros::dec;
use tokio::sync::broadcast;

use super::{
    currency_pair_metadata::CurrencyPairMetadata, currency_pair_metadata::Precision,
    exchange::Exchange,
};
use crate::core::exchanges::binance::binance::BinanceBuilder;
use crate::core::exchanges::events::ExchangeEvent;
use crate::core::exchanges::traits::ExchangeClientBuilder;
use crate::core::lifecycle::application_manager::ApplicationManager;
use crate::core::lifecycle::cancellation_token::CancellationToken;
use crate::core::{
    exchanges::binance::binance::Binance, exchanges::common::Amount,
    exchanges::common::CurrencyPair, exchanges::common::ExchangeAccountId,
    exchanges::common::Price, exchanges::events::AllowedEventSourceType,
    exchanges::general::commission::Commission, exchanges::general::commission::CommissionForType,
    exchanges::general::features::ExchangeFeatures, exchanges::general::features::OpenOrdersType,
    exchanges::timeouts::timeout_manager::TimeoutManager, orders::order::ClientOrderId,
    orders::order::OrderRole, orders::order::OrderSide, orders::order::OrderSnapshot,
    orders::order::OrderType, orders::pool::OrderRef, orders::pool::OrdersPool, settings,
};

pub(crate) fn get_test_exchange(
    is_derivative: bool,
) -> (Arc<Exchange>, broadcast::Receiver<ExchangeEvent>) {
    let exchange_account_id = ExchangeAccountId::new("local_exchange_account_id".into(), 0);
    let mut settings = settings::ExchangeSettings::new_short(
        exchange_account_id.clone(),
        "test_api_key".into(),
        "test_secret_key".into(),
        false,
    );

    let application_manager = ApplicationManager::new(CancellationToken::new());
    let (tx, rx) = broadcast::channel(10);

    BinanceBuilder.extend_settings(&mut settings);
    settings.web_socket_host = "host".into();
    settings.web_socket2_host = "host2".into();

    let binance = Box::new(Binance::new(
        "Binance0".parse().expect("in test"),
        settings.clone(),
        tx.clone(),
        application_manager.clone(),
    ));
    let referral_reward = dec!(40);
    let commission = Commission::new(
        CommissionForType::new(dec!(0.1), referral_reward),
        CommissionForType::new(dec!(0.2), referral_reward),
    );

    let exchange = Exchange::new(
        exchange_account_id,
        binance,
        ExchangeFeatures::new(
            OpenOrdersType::AllCurrencyPair,
            false,
            true,
            AllowedEventSourceType::default(),
            AllowedEventSourceType::default(),
        ),
        tx,
        application_manager,
        TimeoutManager::new(HashMap::new()),
        commission,
    );
    let base_currency_code = "PHB";
    let quote_currency_code = "BTC";
    let amount_currency_code = if is_derivative {
        quote_currency_code.clone()
    } else {
        base_currency_code.clone()
    };

    let price_tick = dec!(0.1);
    let symbol = CurrencyPairMetadata::new(
        false,
        is_derivative,
        base_currency_code.into(),
        base_currency_code.into(),
        quote_currency_code.into(),
        quote_currency_code.into(),
        None,
        None,
        amount_currency_code.into(),
        None,
        None,
        None,
        None,
        Precision::ByTick { tick: price_tick },
        Precision::ByTick { tick: dec!(0) },
    );
    exchange
        .symbols
        .insert(symbol.currency_pair(), Arc::new(symbol));

    (exchange, rx)
}

pub(crate) fn create_order_ref(
    client_order_id: &ClientOrderId,
    role: Option<OrderRole>,
    exchange_account_id: &ExchangeAccountId,
    currency_pair: &CurrencyPair,
    price: Price,
    amount: Amount,
    side: OrderSide,
) -> OrderRef {
    let order = OrderSnapshot::with_params(
        client_order_id.clone(),
        OrderType::Liquidation,
        role,
        exchange_account_id.clone(),
        currency_pair.clone(),
        price,
        amount,
        side,
        None,
        "StrategyInUnitTests",
    );

    let order_pool = OrdersPool::new();
    order_pool.add_snapshot_initial(Arc::new(RwLock::new(order)));
    let order_ref = order_pool
        .cache_by_client_id
        .get(&client_order_id)
        .expect("in test");

    order_ref.clone()
}

pub(crate) fn try_add_snapshot_by_exchange_id(exchange: &Exchange, order_ref: &OrderRef) {
    if let Some(exchange_order_id) = order_ref.exchange_order_id() {
        let _ = exchange
            .orders
            .cache_by_exchange_id
            .insert(exchange_order_id.clone(), order_ref.clone());
    }
}
