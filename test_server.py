import os
import random
import object_storage_proxy as osp

from dotenv import load_dotenv

from object_storage_proxy import start_server, ProxyServerConfig


_TRUES  = {"y", "yes", "t", "true", "on", "1"}
_FALSES = {"n", "no", "f", "false", "off", "0"}


def strtobool(val: str) -> bool:
    """Convert a string to True/False, raise ValueError otherwise."""
    v = val.lower()
    if v in _TRUES:
        return True
    if v in _FALSES:
        return False
    raise ValueError(f"invalid truth value {val!r}")


def docreds(bucket) -> str:
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}...")
    return apikey

def do_validation(token: str, bucket: str) -> bool:
    print(f"PYTHON: Validating headers: {token} for {bucket}...")
    # return random.choice([True, False])
    return True


def main() -> None:
    load_dotenv()

    counting = strtobool(os.getenv("OSP_ENABLE_REQUEST_COUNTING", "false"))

    if counting:
        osp.enable_request_counting()
        print("Request counting enabled")


    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")

    cos_map = {
        "bucket1": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "port": 443,
            "apikey": apikey,
            "ttl": 0
        },
        "bucket2": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "port": 443,
            "apikey": apikey
        },
        "proxy-bucket01": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "access_key": os.getenv("ACCESS_KEY"),
            "secret_key": os.getenv("SECRET_KEY"),
            "port": 443,
            "ttl": 300
        }
    }

    ra = ProxyServerConfig(
        cos_map=cos_map,
        bucket_creds_fetcher=docreds,
        validator=do_validation,
        http_port=6190,
        https_port=8443,
        threads=1,
    )

    start_server(ra)


if __name__ == "__main__":
    main()



