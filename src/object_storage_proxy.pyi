from typing import Any, Mapping, Callable, Optional

class ProxyServerConfig:
    cos_map: Mapping[str, Any]
    bucket_creds_fetcher: Optional[Callable[[str], str]]
    http_port: int
    https_port: int
    validator: Optional[Callable[..., bool]]
    threads: Optional[int]

    def __init__(self, cos_map: Mapping[str, Any], *,
                 bucket_creds_fetcher: Optional[Callable[[str], str]] = ...,
                 http_port: int = 6190,
                 https_port: int = 443,
                 validator: Optional[Callable[..., bool]] = ...,
                 threads: Optional[int] = 1) -> None: ...
    def __repr__(self) -> str: ...

def start_server(run_args: ProxyServerConfig) -> None: ...
def enable_request_counting() -> None: ...
def disable_request_counting() -> None: ...
def get_request_count() -> int: ...
