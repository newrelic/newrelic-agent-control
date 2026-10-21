/** Minimal Java agent used as the `-javaagent` marker, so it cannot be mistaken for the real one. */
public class StubAgent {
  public static void premain(String args) {
    System.out.println("stub-agent: loaded");
  }
}
