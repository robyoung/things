"""A Google Cloud Python Pulumi program"""
import base64
import hashlib
import os

import pulumi
from pulumi_gcp import cloudfunctions, cloudscheduler, pubsub, serviceaccount, storage

from utils import iam_service_account, secret, secret_binding

PATH_TO_SOURCE_CODE = "./functions"

config = pulumi.Config(name=None)

account = serviceaccount.Account(
    "things",
    account_id="things-service-account",
    display_name="Things Service Account",
)

gkeep_username = secret("gkeep-username", config.require_secret("gkeep-username"))
gkeep_password = secret("gkeep-password", config.require_secret("gkeep-password"))
gkeep_token = secret("gkeep-token")

secret_binding(
    "gkeep-username", gkeep_username, account, "roles/secretmanager.secretAccessor"
)
secret_binding(
    "gkeep-password", gkeep_password, account, "roles/secretmanager.secretAccessor"
)
secret_binding(
    "gkeep-token-read", gkeep_token, account, "roles/secretmanager.secretAccessor"
)
secret_binding(
    "gkeep-token-write", gkeep_token, account, "roles/secretmanager.secretVersionAdder"
)
secret_binding("gkeep-token-get", gkeep_token, account, "roles/secretmanager.viewer")

topic = pubsub.Topic("trigger")

job = cloudscheduler.Job(
    "job",
    pubsub_target=cloudscheduler.JobPubsubTargetArgs(
        topic_name=pulumi.Output.all(topic.project, topic.name).apply(
            lambda args: f"projects/{args[0]}/topics/{args[1]}"
        ),
        data=base64.b64encode(b"refresh").decode("utf8"),
    ),
    schedule="*/10 7-22 * * *",
    time_zone="Europe/London",
)

code_bucket = storage.Bucket("things-code", location="EUROPE-WEST1")
data_bucket = storage.Bucket("things-data", location="EUROPE-WEST1")


storage.BucketIAMBinding(
    "things-data",
    bucket=data_bucket.name,
    role="roles/storage.objectAdmin",
    members=[iam_service_account(account)],
)


def archive_hash(archive: pulumi.AssetArchive) -> str:
    hasher = hashlib.sha1()
    for asset in archive.assets.values():
        assert isinstance(asset, pulumi.FileAsset)
        with open(asset.path, "rb") as f:
            hasher.update(f.read())

    return hasher.hexdigest()


# The Cloud Function source code itself needs to be zipped up into an
# archive, which we create using the pulumi.AssetArchive primitive.
archive = pulumi.AssetArchive(
    assets={
        file: pulumi.FileAsset(path=os.path.join(PATH_TO_SOURCE_CODE, file))
        for file in os.listdir(PATH_TO_SOURCE_CODE)
    }
)

# Create the single Cloud Storage object, which contains all of the function's
# source code. ("main.py" and "requirements.txt".)
source_archive_object = storage.BucketObject(
    "things",
    name=f"main.py-{archive_hash(archive)}",
    bucket=code_bucket.name,
    source=archive,
)

# Create the Cloud Function, deploying the source we just uploaded to Google
# Cloud Storage.
fxn = cloudfunctions.Function(
    "things",
    entry_point="trigger",
    environment_variables={
        "GKEEP_USERNAME_KEY": gkeep_username.secret_id,
        "GKEEP_PASSWORD_KEY": gkeep_password.secret_id,
        "GKEEP_TOKEN_KEY": gkeep_token.secret_id,
        "THINGS_BUCKET_NAME": data_bucket.name,
    },
    region="europe-west1",
    runtime="python37",
    service_account_email=account.email,
    source_archive_bucket=code_bucket.name,
    source_archive_object=source_archive_object.name,
    event_trigger=cloudfunctions.FunctionEventTriggerArgs(
        event_type="google.pubsub.topic.publish",
        resource=topic.name,
    ),
    max_instances=1,
)

pulumi.export("data_bucket", data_bucket.url)
pulumi.export("function_name", fxn.name)
