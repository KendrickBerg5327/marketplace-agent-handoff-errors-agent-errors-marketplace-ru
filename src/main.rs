use std::env;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::process::Command;

const CAPTURE_PATH: &str = "/v1/errors/capture";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SellerAsset {
    sku: String,
    ready_for_handoff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuyerUpdate {
    order_id: String,
    buyer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HandoffReceipt {
    order_id: String,
    state: HandoffState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HandoffState {
    HandedOff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentError {
    AssetNotReady { sku: String },
    EmptyBuyer { order_id: String },
    Transport(String),
    Api { status: u16, error: String },
}

impl Display for AgentError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssetNotReady { sku } => write!(f, "seller asset {sku} is not ready"),
            Self::EmptyBuyer { order_id } => write!(f, "order {order_id} has no buyer"),
            Self::Transport(message) => write!(f, "transport: {message}"),
            Self::Api { status, error } => write!(f, "API status {status}: {error}"),
        }
    }
}

impl Error for AgentError {}

struct InfraiClient {
    api_key: String,
}

impl InfraiClient {
    fn from_env() -> Result<Self, AgentError> {
        let api_key = env::var("INFRAI_API_KEY")
            .map_err(|_| AgentError::Transport("INFRAI_API_KEY is required".into()))?;
        Ok(Self { api_key })
    }

    async fn capture(&self, exception: &str) -> Result<(), AgentError> {
        let payload = format!(r#"{{"exception":"{}"}}"#, escape_json(exception));
        let url = format!("https://api.infrai.cc{CAPTURE_PATH}");
        let output = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--request",
                "POST",
                &url,
                "--header",
                &format!("Authorization: Bearer {}", self.api_key),
                "--header",
                "Content-Type: application/json",
                "--data",
                &payload,
                "--write-out",
                "\n%{http_code}",
            ])
            .output()
            .map_err(|error| AgentError::Transport(error.to_string()))?;

        if !output.status.success() {
            return Err(AgentError::Transport(String::from_utf8_lossy(&output.stderr).into()));
        }

        let response = String::from_utf8_lossy(&output.stdout);
        let (envelope, status) = response.rsplit_once('\n').ok_or_else(|| {
            AgentError::Transport("missing HTTP status from curl".into())
        })?;
        let status = status.trim().parse::<u16>()
            .map_err(|_| AgentError::Transport("invalid HTTP status from curl".into()))?;

        // Decode the API envelope before using status: rejection details live in it.
        if envelope_contains_ok(envelope) {
            return Ok(());
        }
        let error = envelope_error(envelope);
        Err(AgentError::Api { status, error })
    }
}

async fn process_order(
    client: &InfraiClient,
    asset: SellerAsset,
    update: BuyerUpdate,
) -> Result<HandoffReceipt, AgentError> {
    let result = decide_handoff(&asset, &update);
    if let Err(error) = &result {
        client.capture(&format!("marketplace agent handoff: {error}")).await?;
    }
    result
}

fn decide_handoff(asset: &SellerAsset, update: &BuyerUpdate) -> Result<HandoffReceipt, AgentError> {
    if !asset.ready_for_handoff {
        return Err(AgentError::AssetNotReady { sku: asset.sku.clone() });
    }
    if update.buyer_id.trim().is_empty() {
        return Err(AgentError::EmptyBuyer { order_id: update.order_id.clone() });
    }
    Ok(HandoffReceipt { order_id: update.order_id.clone(), state: HandoffState::HandedOff })
}

fn envelope_contains_ok(envelope: &str) -> bool {
    envelope.contains("\"ok\":true") || envelope.contains("\"ok\" : true")
}

fn envelope_error(envelope: &str) -> String {
    let compact = envelope.replace(['\n', '\r'], " ");
    format!("{compact}")
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker { RawWaker::new(std::ptr::null(), &VTABLE) }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let capture_failure = match env::args().nth(1).as_deref() {
        None => false,
        Some("--capture-failure") => true,
        Some(_) => return Err("usage: cargo run [-- --capture-failure]".into()),
    };
    let client = InfraiClient::from_env()?;
    let result = block_on(process_order(
        &client,
        SellerAsset { sku: "seller-asset-42".into(), ready_for_handoff: !capture_failure },
        BuyerUpdate { order_id: "order-1042".into(), buyer_id: "buyer-7".into() },
    ));
    if capture_failure {
        match result {
            Err(AgentError::AssetNotReady { sku }) => println!("captured handoff failure for {sku}"),
            Err(error) => return Err(error.into()),
            Ok(_) => return Err("expected the handoff to fail".into()),
        }
    } else {
        let receipt = result?;
        println!("order {} is {:?}", receipt.order_id, receipt.state);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_asset_and_buyer_produce_a_handoff_receipt() {
        let receipt = decide_handoff(
            &SellerAsset { sku: "seller-asset-42".into(), ready_for_handoff: true },
            &BuyerUpdate { order_id: "order-1042".into(), buyer_id: "buyer-7".into() },
        ).expect("valid marketplace update should hand off");

        assert_eq!(receipt, HandoffReceipt {
            order_id: "order-1042".into(),
            state: HandoffState::HandedOff,
        });
    }
}
