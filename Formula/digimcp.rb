class Digimcp < Formula
  desc "DigiiCampus student intelligence CLI and MCP server"
  homepage "https://github.com/kushvinth/DigiMCP"
  version "0.1.1"
  license "MIT"

  on_macos do
    url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-macos-universal.tar.gz"
    sha256 "1f92e3f627facd652077158f47112fc1d32440303670aae95cb235fc0c74ab37"
  end

  on_linux do
    on_arm do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-linux-aarch64.tar.gz"
      sha256 "4cc27dc62dcdb521793f496d493e36def6a9d19473afc2d6a4c49aabd5daa8a0"
    end

    on_intel do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-linux-x86_64.tar.gz"
      sha256 "eaacf62c027d0d89ad7929fa8aa4f677446b008536d7fa0146de57a5fae8f668"
    end
  end

  def install
    bin.install "digi"
    bin.install_symlink bin/"digi" => "digii"
    bin.install_symlink bin/"digi" => "digimcp"
  end

  test do
    assert_match "digi", shell_output("#{bin}/digi --version")
    assert_match "digi", shell_output("#{bin}/digimcp --version")
  end
end
