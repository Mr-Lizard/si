import { parse } from "https://deno.land/std@0.201.0/flags/mod.ts";

export function parseArgs(args: string[]) {
  // Parse arguments using std/flags
  const parsedArgs = parse(args, {
    string: ["workspaceId", "userId", "password", "profile", "tests"],
    alias: { w: "workspaceId", u: "userId", p: "password", t: "tests", l: "profile" },
    default: { profile: undefined, tests: "" },
    boolean: ["help"],
  });

  // Display help information if required arguments are missing or help flag is set
  if (parsedArgs.help || !parsedArgs.workspaceId || !parsedArgs.userId) {
    console.log(`
Usage: deno run main.ts [options]

Options:
  --workspaceId, -w   Workspace ID (required)
  --userId, -u        User ID (required)
  --password, -p      User password (optional)
  --tests, -t         Test names to run (comma-separated, optional)
  --profile, -l       Test profile in JSON format (optional)
  --help              Show this help message
`);
    Deno.exit(0);
  }

  // Extract parsed arguments
  const workspaceId = parsedArgs.workspaceId;
  const userId = parsedArgs.userId;
  const password = parsedArgs.password || undefined;

  // Handle optional tests argument
  const testsToRun = parsedArgs.tests ? parsedArgs.tests.split(",").map(test => test.trim()).filter(test => test) : [];

  // Parse profile JSON if provided, otherwise the profile is one shot [aka single execution]
  let testProfile = "one-shot";
  if (parsedArgs.profile) {
    try {
      testProfile = JSON.parse(parsedArgs.profile);
    } catch (error) {
      throw new Error(`Failed to parse profile JSON: ${error.message}`);
    }
  }

  return { workspaceId, userId, password, testsToRun, testProfile };
}

export function checkEnvironmentVariables(env: Record<string, string | undefined>) {
  const requiredVars = ["SDF_API_URL", "AUTH_API_URL"];
  const missingVars = requiredVars.filter(varName => !env[varName] || env[varName]?.length === 0);
  
  if (missingVars.length > 0) {
    throw new Error(`Missing environment variables: ${missingVars.join(", ")}`);
  }
}