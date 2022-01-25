import datetime
import json
import os

import gkeepapi
from google.api_core.exceptions import NotFound
from google.cloud import secretmanager, storage

secrets_client = secretmanager.SecretManagerServiceClient()

THINGS_BUCKET_NAME = os.environ["THINGS_BUCKET_NAME"]
GKEEP_USERNAME_KEY = os.environ["GKEEP_USERNAME_KEY"]
GKEEP_PASSWORD_KEY = os.environ["GKEEP_PASSWORD_KEY"]
GKEEP_TOKEN_KEY = os.environ["GKEEP_TOKEN_KEY"]
GCP_PROJECT = os.environ["GCP_PROJECT"]

GKeepList = gkeepapi._node.List
GKeepListItem = gkeepapi._node.ListItem


def get_secret(client: secretmanager.SecretManagerServiceClient, name: str) -> str:
    request = {"name": f"projects/{GCP_PROJECT}/secrets/{name}/versions/latest"}
    return client.access_secret_version(request).payload.data.decode("utf8")  # type: ignore


def add_secret_version(
    client: secretmanager.SecretManagerServiceClient, name: str, value: str
) -> None:
    request = {
        "parent": f"projects/{GCP_PROJECT}/secrets/{name}",
        "payload": {"data": value.encode("utf8")},
    }
    client.add_secret_version(request)  # type: ignore


GKEEP_USERNAME = get_secret(secrets_client, GKEEP_USERNAME_KEY)
GKEEP_PASSWORD = get_secret(secrets_client, GKEEP_PASSWORD_KEY)


def login() -> gkeepapi.Keep:
    keep = gkeepapi.Keep()

    try:
        token = get_secret(secrets_client, GKEEP_TOKEN_KEY)
        if keep.resume(GKEEP_USERNAME, token):
            return keep
    except NotFound:
        pass

    if keep.login(GKEEP_USERNAME, GKEEP_PASSWORD):
        add_secret_version(secrets_client, GKEEP_TOKEN_KEY, keep.getMasterToken())
        return keep

    raise ValueError("failed login")


def upload(payload: str) -> None:
    client = storage.Client()
    bucket = client.bucket(THINGS_BUCKET_NAME)
    latest = bucket.blob("unchecked/latest.json")
    try:
        old_items_text = latest.download_as_text()
    except NotFound:
        old_items_text = None

    if old_items_text != payload:
        print(f"uploading new list {payload}")
        latest.upload_from_string(payload)
        stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()[:-13]
        history = bucket.blob(f"unchecked/history/{stamp}.json")
        history.upload_from_string(payload)
    else:
        print("no change")


def main() -> None:
    keep = login()
    shopping = next(keep.find(query="Shopping"))

    items = [
        item
        for item in {item.text.strip().lower() for item in shopping.unchecked}
        if item
    ]
    items.sort()
    items_text = json.dumps(items)

    upload(items_text)


def trigger(event, context):
    main()


if __name__ == "__main__":
    trigger({}, None)
