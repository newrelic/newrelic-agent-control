/** Minimal application that reports whether the New Relic Java agent was attached to its JVM. */
public class SampleApp {
  public static void main(String[] args) {
    System.out.println("JAVA_TOOL_OPTIONS: " + System.getenv("JAVA_TOOL_OPTIONS"));
    System.out.println("NEW_RELIC_APP_NAME: " + System.getenv("NEW_RELIC_APP_NAME"));
    System.out.println("NEWRELIC_FILE: " + System.getenv("NEWRELIC_FILE"));
    System.out.println("agent-attached: " + isAgentAttached());
  }

  // The agent jar is appended to the system class path when it is loaded as a -javaagent.
  private static boolean isAgentAttached() {
    try {
      Class.forName("com.newrelic.bootstrap.BootstrapAgent");
      return true;
    } catch (ClassNotFoundException e) {
      return false;
    }
  }
}
