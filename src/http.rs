use defmt::{Format, error};
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_rp::clocks::RoscRng;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
use reqwless::request::Method;

#[derive(Format)]
pub struct HttpError;

pub async fn http_get<'a, 'b>(
    stack: &Stack<'a>,
    url: &str,
    buf: &'b mut [u8],
) -> Result<&'b [u8], HttpError> {
    let seed = RoscRng.next_u64();

    let mut tls_read_buffer = [0; 16640];
    let mut tls_write_buffer = [0; 16640];
    let dns_client = DnsSocket::new(*stack);

    let client_state = TcpClientState::<1, 1024, 1024>::new();
    let client = TcpClient::<'_, 1>::new(*stack, &client_state);

    let tls_config = TlsConfig::new(
        seed,
        &mut tls_read_buffer,
        &mut tls_write_buffer,
        TlsVerify::None,
    );

    let mut http_client = HttpClient::new_with_tls(&client, &dns_client, tls_config);

    let req = http_client.request(Method::GET, url).await;

    if let Err(e) = req {
        error!("Failed to send HTTP request: {:?}", e);
        return Err(HttpError);
    }

    let mut req = req.unwrap();

    let response = req.send(buf).await;

    if let Err(e) = response {
        error!("Failed to send HTTP request: {:?}", e);
        return Err(HttpError);
    }

    let response = response.unwrap();

    let body_bytes = response.body().read_to_end().await;

    match body_bytes {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            error!("Failed to read HTTP response body: {:?}", e);
            Err(HttpError)
        }
    }
}
