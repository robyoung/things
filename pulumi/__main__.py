"""A Google Cloud Python Pulumi program"""
import base64
import os
import time

from pulumi_gcp import cloudfunctions, cloudscheduler, pubsub, storage

import pulumi

PATH_TO_SOURCE_CODE = "./functions"

config = pulumi.Config(name=None)
gcp_config = pulumi.Config(name="gcp")
config_values = {}


topic = pubsub.Topic("trigger")

project = gcp_config.require("project")

job = cloudscheduler.Job(
    "job",
    pubsub_target=cloudscheduler.JobPubsubTargetArgs(
        topic_name=topic.name.apply(lambda name: f"projects/{project}/topics/{name}"),
        data=base64.b64encode(b"refresh").decode("utf8"),
    ),
    schedule="* * * * *",
    time_zone="Europe/London",
)

# Create a GCP resource (Storage Bucket)
bucket = storage.Bucket("things_bucket", location="EUROPE-WEST1")

# The Cloud Function source code itself needs to be zipped up into an
# archive, which we create using the pulumi.AssetArchive primitive.
assets = {}
for file in os.listdir(PATH_TO_SOURCE_CODE):
    location = os.path.join(PATH_TO_SOURCE_CODE, file)
    asset = pulumi.FileAsset(path=location)
    assets[file] = asset

archive = pulumi.AssetArchive(assets=assets)

# Create the single Cloud Storage object, which contains all of the function's
# source code. ("main.py" and "requirements.txt".)
source_archive_object = storage.BucketObject(
    "things", name="main.py-%f" % time.time(), bucket=bucket.name, source=archive
)

# Create the Cloud Function, deploying the source we just uploaded to Google
# Cloud Storage.
fxn = cloudfunctions.Function(
    "eta_demo_function",
    entry_point="trigger",
    environment_variables=config_values,
    region="europe-west1",
    runtime="python37",
    source_archive_bucket=bucket.name,
    source_archive_object=source_archive_object.name,
    event_trigger=cloudfunctions.FunctionEventTriggerArgs(
        event_type="google.pubsub.topic.publish",
        resource=topic.name,
    ),
)

invoker = cloudfunctions.FunctionIamMember(
    "invoker",
    project=fxn.project,
    region=fxn.region,
    cloud_function=fxn.name,
    role="roles/cloudfunctions.invoker",
    member="allUsers",
)

# Export the DNS name of the bucket
pulumi.export("bucket_name", bucket.url)
pulumi.export("function_name", fxn.name)
