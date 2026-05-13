from typing import Any, Mapping, Callable, Optional, List

HmacKey = Mapping[str, str]  # {"access_key": str, "secret_key": str}

class ProxyServerConfig:
    cos_map: Mapping[str, Any]
    hmac_keystore: Optional[List[HmacKey]]
    bucket_creds_fetcher: Optional[Callable[[str, str], str]]
    http_port: Optional[int]
    https_port: Optional[int]
    validator: Optional[Callable[..., bool]]
    threads: Optional[int]
    verify: Optional[bool]
    skip_signature_validation: Optional[bool]
    hmac_fetcher: Optional[Callable[[str], Optional[str]]]
    max_presign_url_usage_attempts: Optional[int]
    server_name: str
    metrics_port: Optional[int]

    def __init__(
        self,
        cos_map: Mapping[str, Any],
        *,
        hmac_keystore: Optional[List[HmacKey]] = None,
        bucket_creds_fetcher: Optional[Callable[[str, str], str]] = None,
        http_port: Optional[int] = None,
        https_port: Optional[int] = None,
        validator: Optional[Callable[..., bool]] = None,
        threads: Optional[int] = 1,
        verify: Optional[bool] = None,
        skip_signature_validation: Optional[bool] = False,
        hmac_fetcher: Optional[Callable[[str], Optional[str]]] = None,
        max_presign_url_usage_attempts: Optional[int] = 3,
        server_name: str = "osp",
        metrics_port: Optional[int] = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

def start_server(run_args: ProxyServerConfig) -> None: ...
def enable_request_counting() -> None: ...
def disable_request_counting() -> None: ...
def get_request_count() -> int: ...
