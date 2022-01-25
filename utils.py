from typing import Optional

from pulumi_gcp import secretmanager, serviceaccount

from pulumi.output import Output


def secret(
    name: str, secret_data: Optional[Output[str]] = None
) -> secretmanager.Secret:
    s = secretmanager.Secret(
        name,
        secret_id=f"things-{name}",
        replication=secretmanager.SecretReplicationArgs(automatic=True),
    )
    if secret_data is not None:
        secretmanager.SecretVersion(name, secret=s.id, secret_data=secret_data)
    return s


def secret_binding(
    name: str, secret: secretmanager.Secret, account: serviceaccount.Account, role: str
) -> secretmanager.SecretIamBinding:
    return secretmanager.SecretIamBinding(
        name,
        project=secret.project,
        secret_id=secret.secret_id,
        role=role,
        members=[iam_service_account(account)],
    )


def iam_service_account(account: serviceaccount.Account) -> Output[str]:
    return account.email.apply(lambda email: f"serviceAccount:{email}")
