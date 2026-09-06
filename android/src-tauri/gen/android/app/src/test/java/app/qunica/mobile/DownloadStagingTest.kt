package app.qunica.mobile

import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.ByteArrayOutputStream
import java.util.Base64

class DownloadStagingTest {
    @get:Rule val temp = TemporaryFolder()

    @Test fun binaryDownloadSurvivesMultipleChunks() {
        val staging = DownloadStaging(temp.newFolder())
        val bytes = ByteArray(1_100_003) { (it % 256).toByte() }
        val id = staging.begin("报告.pdf", "application/pdf", bytes.size.toLong())
        for (offset in bytes.indices step 65536) {
            staging.append(id, offset.toLong(), Base64.getEncoder().encodeToString(bytes.copyOfRange(offset, minOf(offset + 65536, bytes.size))))
        }
        val result = ByteArrayOutputStream()
        staging.copyTo(id, result)
        assertArrayEquals(bytes, result.toByteArray())
        val file = staging.entry(id).file
        staging.discard(id)
        assertFalse(file.exists())
    }

    @Test fun incompleteAndRepeatedChunksAreRejected() {
        val staging = DownloadStaging(temp.newFolder())
        val id = staging.begin("a", "application/octet-stream", 4)
        staging.append(id, 0, "AQI=")
        assertThrows(IllegalStateException::class.java) { staging.ready(id) }
        assertThrows(IllegalArgumentException::class.java) { staging.append(id, 0, "AQI=") }
        assertThrows(IllegalArgumentException::class.java) { staging.append(id, 2, "AQIDBA==") }
        staging.append(id, 2, "AwQ=")
        assertEquals(4, staging.ready(id).file.length())
    }

    @Test fun emptyFilesAndCancellationAreSupported() {
        val staging = DownloadStaging(temp.newFolder())
        val id = staging.begin("empty.txt", "text/plain", 0)
        val file = staging.ready(id).file
        assertEquals(0, file.length())
        staging.discard(id)
        staging.discard(id)
        assertFalse(file.exists())
    }

    @Test fun untrustedNamesAndIdentifiersCannotChooseCachePaths() {
        val directory = temp.newFolder()
        val staging = DownloadStaging(directory)
        val id = staging.begin("../../private\\报告.pdf", "application/pdf", 0)
        assertEquals("报告.pdf", staging.ready(id).name)
        assertEquals(directory.canonicalFile, staging.ready(id).file.parentFile.canonicalFile)
        assertThrows(IllegalStateException::class.java) { staging.ready("../secret") }
    }

    @Test fun staleFilesAreCleanedOnRestartAndParallelStagingIsBounded() {
        val directory = temp.newFolder()
        val first = DownloadStaging(directory)
        val old = first.entry(first.begin("old", "text/plain", 0)).file
        val restarted = DownloadStaging(directory)
        assertFalse(old.exists())
        repeat(4) { restarted.begin("file", "text/plain", 0) }
        assertThrows(IllegalStateException::class.java) { restarted.begin("extra", "text/plain", 0) }
        restarted.clear()
        assertEquals(0, directory.listFiles()!!.size)
    }
}
