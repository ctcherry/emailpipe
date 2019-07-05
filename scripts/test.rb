require "net/smtp"
require 'pty'
require 'securerandom'

class NonBlockingLineReader
  def initialize(io)
    @io = io
    @lines = []
    @buf = ""
    @eof = false
  end

  def read_line(wait: 2)
    return @lines.shift unless @lines.empty?

    return nil if @eof

    fill_lines(wait: wait)

    return @lines.shift
  end

  private

  def fill_lines(wait: 2)
    again = wait * 4
    while (again > 0) do
      data = @io.read_nonblock(1024, exception: false)
      break unless data == :wait_readable
      sleep 0.250
      again -= 1
    end

    return nil if data == :wait_readable

    if data.nil? # EOF happened, read the rest
      @lines.push(@buf)
      @buf = ""
      @eof = true
      return
    end

    @buf += data
    while line = @buf.slice!(/.*?#{$/}/)
      @lines.push(line)
    end
  end
end

def test(count: 1)
  puts "test(count: #{count})"
  http_port = 9001
  smtp_port = 9002

  http_read, http_write, http_pid = PTY.spawn("curl", "-s", "http://127.0.0.1:#{http_port}/listen")
  http_read = NonBlockingLineReader.new(http_read)
  line = http_read.read_line

  email = line.split(" ").last

  ok = 0
  c = 1
  while c <= count
    random_token = SecureRandom.hex
    random_user = SecureRandom.hex(6)

    smtp = Net::SMTP.new('localhost', smtp_port)
    smtp.start do |conn|
      conn.send_message("Hello this is my message #{random_token}", "#{random_user}@example.com", email)
    end

    buf = ''
    while (line = http_read.read_line) do
      buf += line
    end

    if buf.include?(random_token)
      puts "##{c}: OK"
      ok += 1
    else
      puts "##{c}: ERR"
    end
    c += 1
  end
  return c == ok
ensure
  Process.kill("SIGINT", http_pid)
end

def test_dual(count: 1)
  puts "test_dual(count: #{count})"
  http_port = 9001
  smtp_port = 9002

  http1_read, http1_write, http1_pid = PTY.spawn("curl", "-s", "http://127.0.0.1:#{http_port}/listen")
  http1_read = NonBlockingLineReader.new(http1_read)
  line1 = http1_read.read_line

  email1 = line1.split(" ").last

  http2_read, http2_write, http2_pid = PTY.spawn("curl", "-s", "http://127.0.0.1:#{http_port}/listen")
  http2_read = NonBlockingLineReader.new(http2_read)
  line2 = http2_read.read_line

  email2 = line2.split(" ").last

  c = 1
  while c <= count
    random_token1 = SecureRandom.hex
    random_user1 = SecureRandom.hex(6)

    random_token2 = SecureRandom.hex
    random_user2 = SecureRandom.hex(6)

    smtp1 = Net::SMTP.new('localhost', smtp_port)
    smtp2 = Net::SMTP.new('localhost', smtp_port)

    smtp1.start
    smtp2.start
    smtp1.send_message("Hello this is my message #{random_token1}", "#{random_user1}@example.com", email1)
    smtp2.send_message("Hello this is my message #{random_token2}", "#{random_user2}@example.com", email2)
    smtp1.finish
    smtp2.finish

    buf = ''
    while (line = http1_read.read_line) do
      buf += line
    end

    if buf.include?(random_token1) && !buf.include?(random_token2)
      puts "##{c}-1: OK"
    else
      puts "##{c}-1: ERR"
    end

    buf = ''
    while (line = http2_read.read_line) do
      buf += line
    end

    if buf.include?(random_token2) && !buf.include?(random_token1)
      puts "##{c}-2: OK"
    else
      puts "##{c}-2: ERR"
    end
    c += 1
  end
ensure
  Process.kill("SIGINT", http1_pid)
  Process.kill("SIGINT", http2_pid)
end

def run

  emailpipe_pid = spawn("target/release/emailpipe")

  test(count: 1)
  test(count: 2)
  test_dual(count: 1)
  test_dual(count: 2)

ensure
  Process.kill("SIGINT", emailpipe_pid)
end

run
