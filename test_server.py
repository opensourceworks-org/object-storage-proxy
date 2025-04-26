import json
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


def do_api_creds(token: str, bucket: str) -> str:
    """Fetch credentials (ro, rw, access_denied) for the given bucket, depending on the token. """
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}...")
    return apikey


def do_hmac_creds(token: str, bucket: str) -> str:
    """ Fetch HMAC credentials (ro, rw, access_denied) for the given bucket, depending on the token """
    access_key = os.getenv("ACCESS_KEY")
    secret_key = os.getenv("SECRET_KEY")
    if not access_key or not secret_key:
        raise ValueError("ACCESS_KEY or SECRET_KEY environment variable not set")
        
    print(f"Fetching HMAC credentials for {bucket}...")

    return json.dumps({
        "access_key": access_key,
        "secret_key": secret_key
    })

def lookup_secret_key(access_key: str) -> str | None:
    # get all environment variables ending in ACCESS_KEY
    access_keys = [{key:value} for key, value in os.environ.items() if key.endswith("ACCESS_KEY") and value==access_key ]

    if len(access_keys) > 0:
        access_key_var = next((k for k, v in access_keys[0].items() if v == access_key), None)

        secret_key_var = access_key_var.replace("ACCESS_KEY", "SECRET_KEY")
        return os.getenv(secret_key_var, None)
    else:
        print(f"no access keys found for : {access_key}")


def do_validation(token: str, bucket: str) -> bool:
    """ Authorize the request based on token for the given bucket. 
        You can plug in your own authorization service here.
        The token is the authorization token passed in the request.
        The bucket is the bucket name.
        The function should return True if the request is authorized, False otherwise.
    """

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
            # "access_key": os.getenv("ACCESS_KEY"),
            # "secret_key": os.getenv("SECRET_KEY"),
            "port": 443,
            "ttl": 300
        },
        "proxy-bucket05": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "access_key": os.getenv("PROXY_BUCKET05_ACCESS_KEY"),
            "secret_key": os.getenv("PROXY_BUCKET05_SECRET_KEY"),
            "port": 443,
            "ttl": 300
        },
        "proxy-aws-bucket01": {
            "host": "s3.eu-west-3.amazonaws.com",
            "region": "eu-west-3",
            "access_key": os.getenv("AWS_ACCESS_KEY"),
            "secret_key": os.getenv("AWS_SECRET_KEY"),
            "port": 443,
            "ttl": 300
        }
    }

    hmac_keys= [
        # {
        #     "access_key": os.getenv("LOCAL_ACCESS_KEY"),
        #     "secret_key": os.getenv("LOCAL_SECRET_KEY")
        # },
        {
            "access_key": os.getenv("LOCAL2_ACCESS_KEY"),
            "secret_key": os.getenv("LOCAL2_SECRET_KEY")
        },

    ]

    ra = ProxyServerConfig(
        cos_map=cos_map,
        bucket_creds_fetcher=do_hmac_creds,
        validator=do_validation,
        http_port=6190,
        # https_port=8443,
        threads=1,
        # verify=False,
        hmac_keystore=hmac_keys,
        skip_signature_validation=False,
        hmac_fetcher=lookup_secret_key
    )

    start_server(ra)


if __name__ == "__main__":
    main()



