import base64
import binascii


def hello(request):
    return "hello"


def trigger(event, context):
    print(
        f"Triggered by messageId {context.event_id} published at {context.timestamp} to {context.resource['name']}"
    )

    print(f"hello {event}")

    if "data" in event:
        try:
            data = base64.b64decode(event["data"]).decode("utf8")
        except binascii.Error as e:
            print(f"failed to parse as b64: {e}")
        else:
            print(f"b64 data: {data}")
