import json
import os

import object_storage_proxy as osp
from dotenv import load_dotenv
from object_storage_proxy import ProxyServerConfig, start_server


def do_hmac_creds(token: str, bucket: str) -> str:
    """Return HMAC credentials for the given bucket as a JSON string.

    The token is the access key parsed from the incoming request's
    Authorization header.  You can use it to look up per-client credentials
    from your own secrets store.
    """
    access_key = os.environ["ACCESS_KEY"]
    secret_key = os.environ["SECRET_KEY"]
    return json.dumps({"access_key": access_key, "secret_key": secret_key})


def lookup_secret_key(access_key: str) -> str | None:
    """Resolve the secret key for a given access key.

    The proxy calls this when it needs to verify an incoming HMAC signature.
    Here we resolve it by convention from environment variables named
    <PREFIX>_ACCESS_KEY / <PREFIX>_SECRET_KEY.
    """
    for key, value in os.environ.items():
        if key.endswith("ACCESS_KEY") and value == access_key:
            secret_var = key.replace("ACCESS_KEY", "SECRET_KEY")
            return os.getenv(secret_var)
    return None


def do_validation(token: str, bucket: str, request: dict) -> bool:
    """Authorise the request.

    Return True to allow, False to deny.  You can call your own IAM or
    OAuth2 service here.  Results are cached by (token, bucket) for the
    duration of the TTL configured on the bucket.
    """
    return True


def main() -> None:
    load_dotenv()

    cos_map = {
        # IBM COS bucket using IAM API key auth
        "my-ibm-bucket": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "port": 443,
            "apikey": os.environ["COS_API_KEY"],
            "ttl": 300,
        },
        # AWS bucket using static HMAC credentials
        "my-aws-bucket": {
            "host": "s3.eu-west-3.amazonaws.com",
            "region": "eu-west-3",
            "access_key": os.getenv("AWS_ACCESS_KEY"),
            "secret_key": os.getenv("AWS_SECRET_KEY"),
            "port": 443,
            "ttl": 300,
        },
    }

    hmac_keys = [
        {
            "access_key": os.environ["LOCAL2_ACCESS_KEY"],
            "secret_key": os.environ["LOCAL2_SECRET_KEY"],
        },
    ]

    config = ProxyServerConfig(
        cos_map=cos_map,
        hmac_keystore=hmac_keys,
        bucket_creds_fetcher=do_hmac_creds,
        validator=do_validation,
        hmac_fetcher=lookup_secret_key,
        http_port=6190,
        https_port=8443,
        threads=1,
        skip_signature_validation=False,
    )

    start_server(config)


if __name__ == "__main__":
    main()
