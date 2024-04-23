async function deleteTXTRecords(
  recordName: string,
  zoneId: string,
  apiToken: string,
  email: string
) {
  const baseUrl = `https://api.cloudflare.com/client/v4/zones/${zoneId}/dns_records`;

  // Fetch all DNS records
  const getRecords = async () => {
    const response = await fetch(`${baseUrl}?per_page=1000`, {
      headers: {
        "Content-Type": "application/json",
        "X-Auth-Email": email,
        "X-Auth-Key": apiToken,
      },
    });
    const data = await response.json();
    return data.result.filter(
      (record: any) =>
        record.type === "TXT" && record.name.startsWith(recordName)
    );
  };

  const deleteRecord = async (id: string) => {
    const response = await fetch(`${baseUrl}/${id}`, {
      method: "DELETE",
      headers: {
        "Content-Type": "application/json",
        "X-Auth-Email": email,
        "X-Auth-Key": apiToken,
      },
    });

    if (!response.ok) {
      console.error("Failed to delete record:", await response.text());
      return;
    }

    return response.json();
  };

  const recordsToDelete = await getRecords();
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

const zones = process.argv.slice(2);
const apikey = process.env.CLOUDFLARE_API_KEY;
const email = process.env.CLOUDFLARE_EMAIL;

if (!apikey || !email) {
  console.error("Missing Cloudflare API key or email");
  console.error(
    "Please set CLOUDFLARE_API_KEY and CLOUDFLARE_EMAIL environment variables"
  );
  process.exit(1);
}

if (zones.length === 0) {
  console.error("No zones specified");
  console.error("Usage: clear-unused-dns.ts zone1 zone2 ...");
  process.exit(1);
}

for (const zone of zones) {
  console.log("Deleting records for zone:", zone);
  deleteTXTRecords("_acme-challenge", zone, apikey, email);
}
