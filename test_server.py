from object_storage_proxy import start_server, ProxyServerConfig
from dotenv import load_dotenv
import os
import random


load_dotenv()


def docreds(bucket) -> str:
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}...")
    return apikey

def do_validation(token: str, bucket: str) -> bool:
    print(f"PYTHON: Validating headers: {token} for {bucket}...")
    # return random.choice([True, False])  # pointless now since cached
    return True


def main() -> None:
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")

    cos_map = {
        "bucket1": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "port": 443,
            "apikey": apikey,
            "ttl": 0
        },
        "bucket2": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "port": 443,
            "apikey": apikey
        },
        "proxy-bucket01": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "port": 443,
            "ttl": 300
        }
    }

    ra = ProxyServerConfig(
        cos_map=cos_map,
        bucket_creds_fetcher=docreds,
        validator=do_validation,
        http_port=6190,
        https_port=8443
    )

    start_server(ra)


if __name__ == "__main__":
    main()



