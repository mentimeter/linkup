async function deleteTXTRecords(zoneId: string) {
  const baseUrl = `https://api.cloudflare.com/client/v4/zones/${zoneId}/dns_records`;

  // Fetch all DNS records
  const getRecords = async () => {
    const response = await fetch(`${baseUrl}?per_page=1000`, {
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${api_token}`,
      },
    });
    const data = await response.json();
    return data.result.filter(
      (record: any) =>
        record.type === "TXT" && record.name.startsWith("_acme-challenge")
    );
  };

  const recordsToDelete = await getRecords();
  await doBatchDelete(baseUrl, recordsToDelete);
}

async function deleteTunnelCNAMERecords() {
  const baseUrl = `https://api.cloudflare.com/client/v4/zones/${zone_id}/dns_records`;

  // Fetch all DNS records
  const getRecords = async () => {
    const response = await fetch(`${baseUrl}?per_page=1000`, {
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${api_token}`,
      },
    });
    const data = await response.json();
    return data.result.filter(
      (record: any) =>
        record.type === "CNAME" && record.name.startsWith("tunnel-")
    );
  };

  const recordsToDelete = await getRecords();
  await doBatchDelete(baseUrl, recordsToDelete);
}

async function deleteTunnels() {
  const baseUrl = `https://api.cloudflare.com/client/v4/accounts/${account_id}/cfd_tunnel`;

  // Fetch all tunnels
  const getRecords = async () => {
    const response = await fetch(`${baseUrl}?per_page=1000`, {
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${api_token}`,
      },
    });
    const data = await response.json();
    return data.result.filter(
      (tunnel: any) => tunnel.name.startsWith("tunnel-") && !tunnel.deleted_at
    );
  };

  const recordsToDelete = await getRecords();
  await doBatchDelete(baseUrl, recordsToDelete);
}

async function doBatchDelete(url: string, recordsToDelete: any[]) {
  const deleteRecord = async (id: string) => {
    const response = await fetch(`${url}/${id}`, {
      method: "DELETE",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${api_token}`,
      },
    });

    if (!response.ok) {
      console.error("Failed to delete record:", await response.text());
      return;
    }

    return response.json();
  };

  console.log("Records to delete:", recordsToDelete.length);

  // Batch deletion, 20 records at a time
  const batchDelete = async (records: any[]) => {
    for (let i = 0; i < records.length; i += 20) {
      const batch = records.slice(i, i + 20);
      const deletePromises = batch.map((record) => deleteRecord(record.id));
      const results = await Promise.all(deletePromises);
      results.forEach((result) => console.log("Batch delete result:", result));
    }
  };

  await batchDelete(recordsToDelete);
}

const api_token = process.env.LINKUP_CF_API_TOKEN;
const account_id = process.env.LINKUP_CLOUDFLARE_ACCOUNT_ID;
const zone_id = process.env.LINKUP_CLOUDFLARE_ZONE_ID;

if (!api_token || !account_id || !zone_id) {
  console.error("Missing Cloudflare API Token, Account ID or Zone ID");
  console.error(
    "Please set LINKUP_CF_API_TOKEN, LINKUP_CLOUDFLARE_ACCOUNT_ID, and LINKUP_CLOUDFLARE_ZONE_ID environment variables"
  );
  process.exit(1);
}

deleteTunnelCNAMERecords();
deleteTunnels();

const zones = process.argv.slice(2);
if (zones.length === 0) {
  console.error("No zones specified");
  console.error("Usage: clear-unused-dns.ts zone1 zone2 ...");
  process.exit(1);
}

for (const zone of zones) {
  console.log("Deleting records for zone:", zone);
  deleteTXTRecords(zone);
}
