import datetime
import json
import os
from typing import Optional

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


def _secret_path(name: str) -> str:
    return f"projects/{GCP_PROJECT}/secrets/{name}"


def get_secret(
    client: secretmanager.SecretManagerServiceClient,
    name: str,
    *,
    age_limit: Optional[datetime.timedelta] = None,
) -> Optional[str]:
    request = {"name": f"{_secret_path(name)}/versions/latest"}
    try:
        if age_limit:
            secret = client.get_secret_version(request)
            age = datetime.datetime.now(datetime.timezone.utc) - secret.create_time
            if age > age_limit:
                return None
        return client.access_secret_version(request).payload.data.decode("utf8")  # type: ignore
    except NotFound:
        return None


def add_secret_version(
    client: secretmanager.SecretManagerServiceClient, name: str, value: str
) -> None:
    request = {
        "parent": _secret_path(name),
        "payload": {"data": value.encode("utf8")},
    }
    client.add_secret_version(request)  # type: ignore


GKEEP_USERNAME = get_secret(secrets_client, GKEEP_USERNAME_KEY)
GKEEP_PASSWORD = get_secret(secrets_client, GKEEP_PASSWORD_KEY)


def login() -> gkeepapi.Keep:
    keep = gkeepapi.Keep()

    token = get_secret(
        secrets_client, GKEEP_TOKEN_KEY, age_limit=datetime.timedelta(days=1)
    )
    if token and keep.resume(GKEEP_USERNAME, token):
        print("Login resumed")
        return keep

    if keep.login(GKEEP_USERNAME, GKEEP_PASSWORD):
        print("New login")
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
